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


/// The trait used for messages
///
/// This is the main type of messages that will be exchanged between nodes.
/// This trait is implemented automatically for all types that fulfill the requirements.
pub trait Message: Serialize + Deserialize + Send + Sync + Clone + 'static {}
impl<T> Message for T where T: Serialize + Deserialize + Send + Sync + Clone + 'static {}


/// The trait used for node identifiers
///
/// This trait is implemented automatically for all types that fulfill the requirements.
pub trait NodeId: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}
impl<T> NodeId for T where T: Serialize + Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}


/// The trait used for initialization messages
///
/// This is the type of message that will be exchanged during the initialization phase.
/// It needs to contain the [`NodeId`](trait.NodeId.html) of the sending node to indetify it.
/// This trait is implemented automatically for all types that fulfill the requirements.
pub trait InitMessage: Serialize + Deserialize + Send + Sync + Clone + 'static {}
impl<T> InitMessage for T where T: Serialize + Deserialize + Send + Sync + Clone + 'static {}


/// The error type used througout the crate
#[derive(Debug)]
pub enum Error<N> where N: NodeId {
    /// The node has already been closed
    AlreadyClosed,

    /// Failed to open a server socket
    OpenError(IoError),

    /// Failed to establish a connection
    ConnectionError(IoError),

    /// Failed to send a message
    SendError,

    /// Failed to receive a message
    ReadError,

    /// Failed to send a message because there is no connection to the destination
    NotConnected(N),

    /// Connection aborted in the initialization phase
    ConnectionAborted,

    /// Failed to close a socket
    CloseError(IoError)
}


/// The trait used as callback
pub trait Callback<M: Message, N: NodeId, I: InitMessage>: Send + Sync {
    /// The identifier of the current node
    fn node_id(&self, node: &Node<M, N, I>) -> N;

    /// Create an initialization message to be sent to a new connection
    ///
    /// This method is called whenever a new connection is going to be established and an
    /// initialization message needs to be sent.
    fn create_init_msg(&self, node: &Node<M, N, I>) -> I;

    /// Handle an initialization message received from a new connection
    ///
    /// This method is called whenever a new connection is established (either incoming or
    /// outgoing) and the initialization message has been received.
    /// The result of this method must either be the identifier of the remote node if the
    /// connection is accepted or `None` if the connection is rejected.
    ///
    /// Note: The connection has not been registered in the node and can't be used to send any
    /// messages yet.
    fn handle_init_msg(&self, node: &Node<M, N, I>, init: &I) -> Option<N>;

    /// Handle an incoming message
    ///
    /// This method is called whenever a new message is received from any connection.
    fn handle_message(&self, node: &Node<M, N, I>, src: &N, msg: M);

    /// Handle a new connection
    ///
    /// This method is called whenever a new connection has been established and can be used to
    /// send messages. The default is to ignore the event and there is no implementation required.
    #[allow(unused_variables)]
    fn on_connected(&self, node: &Node<M, N, I>, id: &N) {

    }

    /// Handle a lost connection
    ///
    /// This method is called whenever a connection has been lost and can no longer be used to
    /// send messages. The default is to ignore the event and there is no implementation required.
    ///
    /// Note: This method is called after the connection has been closed and unregistered,
    /// so it can't be used to send final messages on that connection.
    ///
    /// Note 2: This method will not be called if a duplicate connection to the same node is
    /// closed.
    #[allow(unused_variables)]
    fn on_disconnected(&self, node: &Node<M, N, I>, id: &N) {

    }

    /// The connection timeout
    ///
    /// This method id called to determine the timeout used for connections. Whenever a connection
    /// is idle longer than this timeout, it will be closed. The default value is 60 seconds.
    #[allow(unused_variables)]
    fn connection_timeout(&self, node: &Node<M, N, I>) -> Duration {
        Duration::from_secs(60)
    }

    /// The statistics halflife time
    ///
    /// This value is used as a parameter for the rolling average statistics of sending and
    /// receiving rates. Shorter durations react faster to changes and longer durations result in
    /// smoother behavior. The default value is 60 seconds.
    #[allow(unused_variables)]
    fn stats_halflife_time(&self, node: &Node<M, N, I>) -> Duration {
        Duration::from_secs(60)
    }
}

/// A guard that closes the node when dropped.
///
/// Implementation details require the node to exist in multiple reference counted copies.
/// Therefore this wrapper is needed to implement the drop dehavior.
/// Except for this drop behavior this stuct can be used like the node struct that it encapsulates.
pub struct CloseGuard<M: Message, N: NodeId, I: InitMessage>(Node<M, N, I>);

impl<M: Message, N: NodeId, I: InitMessage> Drop for CloseGuard<M, N, I> {
    fn drop(&mut self) {
        self.close().expect("Failed to close node");
    }
}

impl<M: Message, N: NodeId, I: InitMessage> Deref for CloseGuard<M, N, I> {
    type Target = Node<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}


/// Node statistics
pub struct NodeStats<N: NodeId> {
    /// Statistics for all connections
    pub connections: HashMap<N, ConnectionStats>
}


/// The inner struct that holds all node data
pub struct NodeInner<M: Message, N: NodeId, I: InitMessage> {
    callback: Box<Callback<M, N, I>>,
    sockets: Mutex<Vec<(Arc<TcpListener>, JoinHandle<Result<(), Error<N>>>)>>,
    connections: RwLock<HashMap<N, Connection<M, N, I>>>,
    closed: RwLock<bool>,
}


/// The node struct
///
/// This is a reference counted wrapper around the node data.
/// This struct can be parametrized with the following types:
///
/// * `M` a type implementing the [`Message`](trait.Message.html) trait that is used for all
///   messages that are exchanged.
/// * `N` a type implementing the [`NodeId`](trait.NodeId.html) trait that is used to identify and
///   distinguish nodes.
/// * `I` a type implementing the [`InitMessage`](trait.InitMessage.html) trait that is used for
///   the first initialization message exchanged on any connection.
#[derive(Clone)]
pub struct Node<M: Message, N: NodeId, I: InitMessage>(Arc<NodeInner<M, N, I>>);

impl<M: Message, N: NodeId, I: InitMessage> Deref for Node<M, N, I> {
    type Target = NodeInner<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: Message, N: NodeId, I: InitMessage> Node<M, N, I> {
    /// Create a new node
    ///
    /// The only parameter `callback` will be used to communicate with the caller.
    /// The result of this call is a guard that closes the node if dropped.
    pub fn new(callback: Box<Callback<M, N, I>>) -> CloseGuard<M, N, I> {
        CloseGuard(Node(Arc::new(NodeInner{
            callback: callback,
            sockets: Mutex::new(Vec::new()),
            connections: RwLock::new(HashMap::new()),
            closed: RwLock::new(false)
        })))
    }

    /// Listen on the specified address
    ///
    /// This method will open a new server listening to the given address. A dedicated thread will
    /// be started to handle incoming connections. The node can listen on multiple addresses.
    /// The result of this method, if successfull, is the actual address used.
    ///
    /// Note: a port of `0` has the special meaning of taking a random free port.
    pub fn listen<A: ToSocketAddrs>(&self, addr: A) -> Result<SocketAddr, Error<N>> {
        if *self.closed.read().expect("Lock poisoned") {
            return Err(Error::AlreadyClosed);
        }
        let mut servers = self.sockets.lock().expect("Lock poisoned");
        let server: Arc<TcpListener> = Arc::new(try!(TcpListener::bind(addr).map_err(|err| Error::OpenError(err))));
        let cloned_self = self.clone();
        let cloned_server = server.clone();
        let used_addr = server.local_addr().expect("Failed to get local address");
        let join = thread::spawn(move || cloned_self.run_server(cloned_server));
        servers.push((server, join));
        Ok(used_addr)
    }

    /// Listen on the addresses `"0.0.0.0:0"` and `[::0]:0`
    ///
    /// This will open sockets for IPv4 and IPv6 (see [`listen`](struct.Node.html#method.listen)
    /// for details).
    ///
    /// Note: This method will always open two new sockets, regardless of whether sockets are
    /// already open.
    pub fn listen_defaults(&self) -> Result<(), Error<N>> {
        try!(self.listen("0.0.0.0:0"));
        try!(self.listen("[::0]:0"));
        Ok(())
    }

    /// The addresses of this node
    pub fn addresses(&self) -> Vec<SocketAddr> {
        let mut addrs = Vec::new();
        for &(ref sock, _) in &self.sockets.lock().expect("Lock poisoned") as &Vec<(Arc<TcpListener>, _)> {
            addrs.push(sock.local_addr().expect("Failed to obtain address"));
        }
        addrs
    }

    /// The id of this node as obtained from the callback
    pub fn node_id(&self) -> N {
        self.callback.node_id(&self)
    }

    fn create_init_msg(&self) -> I {
        self.callback.create_init_msg(&self)
    }

    fn handle_init_msg(&self, init: &I) -> Option<N> {
        self.callback.handle_init_msg(&self, &init)
    }

    /// The connection timeout as obtained from the callback
    pub fn connection_timeout(&self) -> Duration {
        self.callback.connection_timeout(&self)
    }

    /// The statistics halflife time as obtained from the callback
    pub fn stats_halflife_time(&self) -> Duration {
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

    /// Whether this node is connected to the given node
    pub fn is_connected(&self, id: &N) -> bool {
        self.connections.read().expect("Lock poisoned").contains_key(id)
    }

    fn get_connections(&self) -> Vec<Connection<M, N, I>> {
        self.connections.read().expect("Lock poisoned").values().map(|c| c.clone()).collect()
    }

    /// The statistics of this node
    pub fn stats(&self) -> NodeStats<N> {
        let mut stats = NodeStats{connections: HashMap::new()};
        for (id, con) in self.connections.read().expect("Lock poisoned").iter() {
            stats.connections.insert(id.clone(), con.stats());
        }
        stats
    }

    fn run_server(&self, socket: Arc<TcpListener>) -> Result<(), Error<N>> {
        loop {
            let (sock, _) = try!(socket.accept().map_err(|e| Error::ConnectionError(e)));
            let con = try!(Connection::new(self.clone(), sock));
            self.add_connection(con.clone());
            thread::spawn(move || con.run());
        }
    }

    /// Send a message
    ///
    /// This method sends a message to the given destination. The message must be encodable using
    /// serde and the node must be connected to the destination.
    /// It is possible to send messages to the node itself by using its address.
    /// If no connection to the destination exists, an error is returned.
    #[inline]
    pub fn send(&self, dst: N, msg: &M) -> Result<(), Error<N>> {
        if dst == self.node_id() {
            self.handle_message(&dst, msg.clone());
            return Ok(());
        }
        match self.get_connection(&dst) {
            Some(con) => con.send(msg),
            None => Err(Error::NotConnected(dst))
        }
    }

    /// Open a connection
    ///
    /// This method opens a connection to the given address, exchanges initialization messages with
    /// it and adds the connection to the registry to be used for sending messages and spawns a
    /// thread to handle incoming messages.
    /// This method will only return after the connection has been fully established.
    /// The result of the method on success is the node id of the new remote node.
    ///
    /// Note: This connection will automatically be closed when it becomes idle for longer than the
    /// [specified timeout](trait.Callback.html#method.connection_timeout).
    ///
    /// Note 2: It is possible to connect the node to itself. However, messages will just be
    /// short-circuited and the connection will not be used and closed after the timeout.
    #[inline]
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<N, Error<N>> {
        if *self.closed.read().expect("Lock poisoned") {
            return Err(Error::AlreadyClosed);
        }
        let sock = try!(TcpStream::connect(addr).map_err(|err| Error::ConnectionError(err)));
        let con = try!(Connection::new(self.clone(), sock));
        let id = con.node_id().clone();
        self.add_connection(con.clone());
        thread::spawn(move || con.run());
        Ok(id)
    }

    fn shutdown_socket(&self, socket: &TcpListener) -> Result<(), Error<N>> {
        //TODO: Remove this workaround once a proper API is available
        let socket = unsafe { mem::transmute::<&TcpListener, &TcpStream>(socket) };
        socket.shutdown(Shutdown::Both).map_err(|e| Error::CloseError(e))
    }

    #[inline]
    fn close(&self) -> Result<(), Error<N>> {
        *self.closed.write().expect("Lock poisoned") = true;
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


/// Statistics of one connection
pub struct ConnectionStats {
    /// The total amount of bytes written
    pub write_total: u64,

    /// The rate of bytes written per second
    pub write_rate: f64,

    /// The current idle time of writing side
    pub write_idle: Duration,

    /// The total amount of bytes read
    pub read_total: u64,

    /// The rate of bytes read per second
    pub read_rate: f64,

    /// The current idle time of reading side
    pub read_idle: Duration
}


pub struct ConnectionInner<M: Message, N: NodeId, I: InitMessage> {
    server: Node<M, N, I>,
    socket: Mutex<TcpStream>,
    writer: Mutex<StatWriter<TcpStream>>,
    writer_stats: Arc<RwLock<Stats>>,
    reader: Mutex<rmp_serde::Deserializer<StatReader<TcpStream>>>,
    reader_stats: Arc<RwLock<Stats>>,
    node_id: N
}

#[derive(Clone)]
pub struct Connection<M: Message, N: NodeId, I: InitMessage>(Arc<ConnectionInner<M, N, I>>);

impl<M: Message, N: NodeId, I: InitMessage> Deref for Connection<M, N, I> {
    type Target = ConnectionInner<M, N, I>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<M: Message, N: NodeId, I: InitMessage> Connection<M, N, I> {
    fn new(server: Node<M, N, I>, mut socket: TcpStream) -> Result<Self, Error<N>> {
        try!(socket.set_nodelay(true).map_err(|err| Error::ConnectionError(err)));
        try!(socket.set_read_timeout(Some(server.connection_timeout())).map_err(|err| Error::ConnectionError(err)));
        {
            let mut writer = rmp_serde::Serializer::new(&mut socket);
            let init = server.create_init_msg();
            try!(init.serialize(&mut writer).map_err(|_| Error::SendError));
        }
        let init = {
            let mut reader = rmp_serde::Deserializer::new(&socket);
            try!(I::deserialize(&mut reader).map_err(|_| Error::ReadError))
        };
        let node_id = match server.handle_init_msg(&init) {
            Some(node_id) => node_id,
            None => return Err(Error::ConnectionAborted)
        };
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
        msg.serialize(&mut writer).map_err(|_| Error::SendError)
    }

    fn run(&self) -> Result<(), Error<N>> {
        let res = self.run_inner();
        self.server.del_connection(&self.node_id);
        res
    }

    fn run_inner(&self) -> Result<(), Error<N>> {
        let mut reader = self.reader.lock().expect("Lock poisoned");
        loop {
            let msg = try!(M::deserialize(&mut reader as &mut rmp_serde::Deserializer<StatReader<TcpStream>>).map_err(|_| Error::ReadError));
            self.server.handle_message(&self.node_id, msg);
        }
    }

    fn close(&self) -> Result<(), Error<N>> {
        Ok(try!(self.socket.lock().expect("Lock poisoned").shutdown(Shutdown::Both).map_err(|err| Error::CloseError(err))))
    }
}
