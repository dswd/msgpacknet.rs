use serde::{Serialize, Deserialize};
use net2::TcpStreamExt;
use rmp_serde;

use std::net::{TcpListener, TcpStream, ToSocketAddrs, Shutdown, SocketAddr};
use std::sync::{RwLock, Arc, Mutex};
use std::collections::HashMap;
use std::ops::Deref;
use std::fmt::Debug;
use std::hash::Hash;
use std::mem;
use std::time::Duration;
use std::thread::{self, JoinHandle};
use std::io::Error as IoError;
use std::io::BufWriter;

use super::stats::{Stats, StatReader, StatWriter};


pub trait Message: Serialize + Deserialize + Send + Sync + Clone + 'static {}
impl<T> Message for T where T: Serialize + Deserialize + Send + Sync + Clone + 'static {}

pub trait NodeId: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}
impl<T> NodeId for T where T: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}

pub trait InitMsg: Serialize + Deserialize + Send + Sync + Clone + 'static {}
impl<T> InitMsg for T where T: Serialize + Deserialize + Send + Sync + Clone + 'static {}


#[derive(Debug)]
pub enum Error<N> where N: NodeId {
    BindError(IoError),
    AcceptError(IoError),
    SocketOptionError(IoError),
    SerializeError,
    DeserializeError,
    UnreachableDestination(N),
    ConnectError(IoError),
    CloseServer(IoError),
    CloseConnection(IoError),
    DuplicateConnection(N),
    InvalidProtocol
}

pub trait Callback<M: Message, N: NodeId, I: InitMsg>: Send + Sync {
    fn node_id(&self, &Node<M, N, I>) -> N;
    fn create_init_msg(&self, &Node<M, N, I>) -> I;
    fn handle_init_msg(&self, &Node<M, N, I>, &I) -> Result<N, Error<N>>;
    fn handle_message(&self, &Node<M, N, I>, &N, M);
    fn on_connected(&self, &Node<M, N, I>, &N);
    fn on_disconnected(&self, &Node<M, N, I>, &N);
    fn connection_timeout(&self, &Node<M, N, I>) -> Duration {
        Duration::from_secs(60)
    }
    fn stats_halflife_time(&self, &Node<M, N, I>) -> Duration {
        Duration::from_secs(60)
    }
}

pub struct CloseGuard<M: Message, N: NodeId, I: InitMsg>(Node<M, N, I>);

impl<M: Message, N: NodeId, I: InitMsg> Drop for CloseGuard<M, N, I> {
    fn drop(&mut self) {
        self.close().expect("Failed to close node");
    }
}

impl<M: Message, N: NodeId, I: InitMsg> Deref for CloseGuard<M, N, I> {
    type Target = Node<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct NodeStats<N: NodeId> {
    pub connections: HashMap<N, ConnectionStats>
}


pub struct NodeInner<M: Message, N: NodeId, I: InitMsg> {
    callback: Box<Callback<M, N, I>>,
    sockets: Mutex<Vec<(Arc<TcpListener>, JoinHandle<Result<(), Error<N>>>)>>,
    connections: RwLock<HashMap<N, Connection<M, N, I>>>,
}

#[derive(Clone)]
pub struct Node<M: Message, N: NodeId, I: InitMsg>(Arc<NodeInner<M, N, I>>);

impl<M: Message, N: NodeId, I: InitMsg> Deref for Node<M, N, I> {
    type Target = NodeInner<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: Message, N: NodeId, I: InitMsg> Node<M, N, I> {
    pub fn new(callback: Box<Callback<M, N, I>>) -> CloseGuard<M, N, I> {
        CloseGuard(Node(Arc::new(NodeInner{
            callback: callback,
            sockets: Mutex::new(Vec::new()),
            connections: RwLock::new(HashMap::new()),
        })))
    }

    pub fn open<A: ToSocketAddrs>(&self, addr: A) -> Result<(), Error<N>> {
        let mut servers = self.sockets.lock().expect("Lock poisoned");
        let server: Arc<TcpListener> = Arc::new(try!(TcpListener::bind(addr).map_err(|err| Error::BindError(err))));
        let cloned_self = self.clone();
        let cloned_server = server.clone();
        let join = thread::spawn(move || cloned_self.run_server(cloned_server));
        servers.push((server, join));
        Ok(())
    }

    pub fn addresses(&self) -> Vec<SocketAddr> {
        let mut addrs = Vec::new();
        for &(ref sock, _) in &self.sockets.lock().expect("Lock poisoned") as &Vec<(Arc<TcpListener>, _)> {
            addrs.push(sock.local_addr().expect("Failed to obtain address"));
        }
        addrs
    }

    fn node_id(&self) -> N {
        self.callback.node_id(&self)
    }

    fn create_init_msg(&self) -> I {
        self.callback.create_init_msg(&self)
    }

    fn handle_init_msg(&self, init: &I) -> Result<N, Error<N>> {
        self.callback.handle_init_msg(&self, &init)
    }

    fn connection_timeout(&self) -> Duration {
        self.callback.connection_timeout(&self)
    }

    fn stats_halflife_time(&self) -> Duration {
        self.callback.stats_halflife_time(&self)
    }

    fn handle_message(&self, src: &N, msg: M) {
        self.callback.handle_message(&self, &src, msg)
    }

    fn add_connection(&self, con: Connection<M, N, I>) {
        let id = con.node_id().clone();
        self.connections.write().expect("Lock poisoned").insert(id.clone(), con);
        self.callback.on_connected(&self, &id);
    }

    fn del_connection(&self, id: &N) {
        self.connections.write().expect("Lock poisoned").remove(id);
        self.callback.on_connected(&self, id);
    }

    fn get_connection(&self, id: &N) -> Option<Connection<M, N, I>> {
        self.connections.read().expect("Lock poisoned").get(id).map(|v| v.clone())
    }

    pub fn is_connected(&self, id: &N) -> bool {
        self.connections.read().expect("Lock poisoned").contains_key(id)
    }

    fn get_connections(&self) -> Vec<Connection<M, N, I>> {
        self.connections.read().expect("Lock poisoned").values().map(|c| c.clone()).collect()
    }

    pub fn stats(&self) -> NodeStats<N> {
        let mut stats = NodeStats{connections: HashMap::new()};
        for (id, con) in self.connections.read().expect("Lock poisoned").iter() {
            stats.connections.insert(id.clone(), con.stats());
        }
        stats
    }

    fn run_server(&self, socket: Arc<TcpListener>) -> Result<(), Error<N>> {
        loop {
            let (sock, _) = try!(socket.accept().map_err(|e| Error::AcceptError(e)));
            let con = try!(Connection::new(self.clone(), sock));
            self.add_connection(con.clone());
            thread::spawn(move || con.run());
        }
    }

    #[inline]
    pub fn send(&self, dst: N, msg: &M) -> Result<(), Error<N>> {
        if dst == self.node_id() {
            self.handle_message(&dst, msg.clone());
            return Ok(());
        }
        match self.get_connection(&dst) {
            Some(con) => con.send(msg),
            None => Err(Error::UnreachableDestination(dst))
        }
    }

    #[inline]
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<(), Error<N>> {
        let sock = try!(TcpStream::connect(addr).map_err(|err| Error::ConnectError(err)));
        let con = try!(Connection::new(self.clone(), sock));
        self.add_connection(con.clone());
        thread::spawn(move || con.run());
        Ok(())
    }

    fn shutdown_socket(&self, socket: &TcpListener) -> Result<(), Error<N>> {
        //TODO: Remove this workaround once a proper API is available
        let socket = unsafe { mem::transmute::<&TcpListener, &TcpStream>(socket) };
        socket.shutdown(Shutdown::Both).map_err(|e| Error::CloseServer(e))
    }

    #[inline]
    pub fn close(&self) -> Result<(), Error<N>> {
        let mut sockets = self.sockets.lock().expect("Lock poisoned");
        while let Some((s, j)) = sockets.pop() {
            try!(self.shutdown_socket(&s));
            j.join().expect("Failed to join").ok();
        }
        for c in self.get_connections() {
            let _ = c.close();
        }
        Ok(())
    }
}


pub struct ConnectionStats {
    pub write_total: u64,
    pub write_rate: f64,
    pub write_idle: Duration,
    pub read_total: u64,
    pub read_rate: f64,
    pub read_idle: Duration
}


pub struct ConnectionInner<M: Message, N: NodeId, I: InitMsg> {
    server: Node<M, N, I>,
    socket: Mutex<TcpStream>,
    writer: Mutex<StatWriter<TcpStream>>,
    writer_stats: Arc<RwLock<Stats>>,
    reader: Mutex<rmp_serde::Deserializer<StatReader<TcpStream>>>,
    reader_stats: Arc<RwLock<Stats>>,
    node_id: N
}

#[derive(Clone)]
pub struct Connection<M: Message, N: NodeId, I: InitMsg>(Arc<ConnectionInner<M, N, I>>);

impl<M: Message, N: NodeId, I: InitMsg> Deref for Connection<M, N, I> {
    type Target = ConnectionInner<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: Message, N: NodeId, I: InitMsg> Connection<M, N, I> {
    fn new(server: Node<M, N, I>, mut socket: TcpStream) -> Result<Self, Error<N>> {
        try!(socket.set_nodelay(true).map_err(|err| Error::SocketOptionError(err)));
        try!(socket.set_read_timeout(Some(server.connection_timeout())).map_err(|err| Error::SocketOptionError(err)));
        {
            let mut writer = rmp_serde::Serializer::new(&mut socket);
            let init = server.create_init_msg();
            try!(init.serialize(&mut writer).map_err(|_| Error::SerializeError));
        }
        let init = {
            let mut reader = rmp_serde::Deserializer::new(&socket);
            try!(I::deserialize(&mut reader).map_err(|_| Error::DeserializeError))
        };
        let node_id = try!(server.handle_init_msg(&init));
        let writer = StatWriter::new(socket.try_clone().expect("Failed to clone socket"), server.stats_halflife_time());
        let writer_stats = writer.stats();
        let input = StatReader::new(socket.try_clone().expect("Failed to clone socket"), server.stats_halflife_time());
        let reader_stats = input.stats();
        let reader = rmp_serde::Deserializer::new(input);
        Ok(Connection(Arc::new(ConnectionInner{
            server: server,
            writer: Mutex::new(writer),
            writer_stats: writer_stats,
            reader: Mutex::new(reader),
            reader_stats: reader_stats,
            socket: Mutex::new(socket),
            node_id: node_id
        })))
    }

    fn node_id(&self) -> &N {
        &self.node_id
    }

    fn stats(&self) -> ConnectionStats {
        let reader_stats = self.reader_stats.read().expect("Lock poisoned");
        let writer_stats = self.writer_stats.read().expect("Lock poisoned");
        ConnectionStats{
            write_total: writer_stats.total(),
            write_rate: writer_stats.rate(),
            write_idle: writer_stats.idle_time(),
            read_total: reader_stats.total(),
            read_rate: reader_stats.rate(),
            read_idle: reader_stats.idle_time()
        }
    }

    fn send(&self, msg: &M) -> Result<(), Error<N>> {
        let mut lock = self.writer.lock().expect("Lock poisoned");
        let mut bufwriter = BufWriter::new(&mut lock as &mut StatWriter<TcpStream>);
        let mut writer = rmp_serde::Serializer::new(&mut bufwriter);
        msg.serialize(&mut writer).map_err(|_| Error::SerializeError)
    }

    fn run(&self) -> Result<(), Error<N>> {
        let res = self.run_inner();
        self.server.del_connection(&self.node_id);
        res
    }

    fn run_inner(&self) -> Result<(), Error<N>> {
        let mut reader = self.reader.lock().expect("Lock poisoned");
        loop {
            let msg = try!(M::deserialize(&mut reader as &mut rmp_serde::Deserializer<StatReader<TcpStream>>).map_err(|_| Error::DeserializeError));
            self.server.handle_message(&self.node_id, msg);
        }
    }

    fn close(&self) -> Result<(), Error<N>> {
        Ok(try!(self.socket.lock().expect("Lock poisoned").shutdown(Shutdown::Both).map_err(|err| Error::CloseConnection(err))))
    }
}
