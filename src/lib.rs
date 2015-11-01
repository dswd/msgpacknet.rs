#![cfg_attr(test, feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
#[cfg(test)] extern crate test;

#[cfg(test)] mod tests;

use serde::{Serialize, Deserialize};
use net2::TcpStreamExt;

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

pub trait Message: Serialize + Deserialize + Send + Sync + Debug + Clone + 'static {}
impl<T> Message for T where T: Serialize + Deserialize + Send + Sync + Debug + Clone + 'static {}

pub trait NodeId: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}
impl<T> NodeId for T where T: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}

pub trait InitMsg: Serialize + Deserialize + Send + Sync + Debug + Clone + 'static {}
impl<T> InitMsg for T where T: Serialize + Deserialize + Send + Sync + Debug + Clone + 'static {}


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


pub struct ConnectionInner<M: Message, N: NodeId, I: InitMsg> {
    server: Node<M, N, I>,
    socket: RwLock<TcpStream>,
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
        Ok(Connection(Arc::new(ConnectionInner{server: server, socket: RwLock::new(socket), node_id: node_id})))
    }

    fn node_id(&self) -> &N {
        &self.node_id
    }

    fn send(&self, msg: &M) -> Result<(), Error<N>> {
        let mut lock = self.socket.write().expect("Lock poisoned");
        let mut bufwriter = BufWriter::new(&mut lock as &mut TcpStream);
        let mut writer = rmp_serde::Serializer::new(&mut bufwriter);
        msg.serialize(&mut writer).map_err(|_| Error::SerializeError)
    }

    fn run(&self) -> Result<(), Error<N>> {
        let res = self.run_inner();
        self.server.del_connection(&self.node_id);
        res
    }

    fn run_inner(&self) -> Result<(), Error<N>> {
        let input = self.socket.read().expect("Lock poisoned").try_clone().expect("Failed to clone socket");
        let mut reader = rmp_serde::Deserializer::new(input);
        loop {
            let msg = try!(M::deserialize(&mut reader).map_err(|_| Error::DeserializeError));
            self.server.handle_message(&self.node_id, msg);
        }
    }

    fn close(&self) -> Result<(), Error<N>> {
        Ok(try!(self.socket.read().expect("Lock poisoned").shutdown(Shutdown::Both).map_err(|err| Error::CloseConnection(err))))
    }
}
