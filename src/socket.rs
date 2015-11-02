use serde::{Serialize, Deserialize};
use net2::TcpStreamExt;
use rmp_serde;
use rand::{self, Rand};

use std::net::{TcpListener, TcpStream, ToSocketAddrs, Shutdown, SocketAddr};
use std::sync::{RwLock, Arc, Mutex};
use std::collections::HashMap;
use std::ops::Deref;
use std::fmt::Debug;
use std::hash::Hash;
use std::{mem, fmt};
use std::time::Duration;
use std::thread::{self, JoinHandle};
use std::io::{self, BufWriter};

use super::stats::{Stats, StatReader, StatWriter};
use super::queue::Queue;


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
/// It could be used to differenciate between different protocols, versions and capabilities and
/// to decide whether to accept a connection or not.
/// This trait is implemented automatically for all types that fulfill the requirements.
pub trait InitMessage: Serialize + Deserialize + Send + Sync + Clone + Debug + 'static {}
impl<T> InitMessage for T where T: Serialize + Deserialize + Send + Sync + Clone + Debug + 'static {}


/// The error type used througout the crate
#[derive(Debug)]
pub enum Error<N> where N: NodeId {
    /// The node has already been closed
    AlreadyClosed,

    /// Failed to open a node socket
    OpenError(io::Error),

    /// Failed to establish a connection
    ConnectionError(io::Error),

    /// Failed to send a message
    SendError,

    /// Failed to receive a message
    ReadError,

    /// Failed to send a message because there is no connection to the destination
    NotConnected(N),

    /// Connection aborted in the initialization phase
    ConnectionAborted,

    /// Failed to close a socket
    CloseError(io::Error)
}


/// The enum used for events
#[derive(PartialEq, Debug)]
pub enum Event<M: Message, N: NodeId, I: InitMessage> {
    /// A received message
    ///
    /// This event is emitted whenever a new message is received from any connection.
    Message(N, M),

    /// An incoming connection request
    ///
    /// This event is emitted whenever a new incoming connection is established and the
    /// initialization message has been received.
    /// To accept the connection [`accept(...)`](struct.ConnectionRequest.html#method.accept)
    /// has to be called.
    ///
    /// Note: The connection has not been registered in the node and can't be used to send any
    ///       messages yet.
    ConnectionRequest(ConnectionRequest<M, N, I>),

    /// A new estasblished connection
    ///
    /// This event is emitted whenever a new connection has been established and can be used to
    /// send messages.
    Connected(N),

    /// A lost connection
    ///
    /// This event is emitted whenever a connection has been lost and can no longer be used to
    /// send messages.
    ///
    /// Note: This event is emitted after the connection has been closed and unregistered,
    ///       so it can't be used to send final messages on that connection.
    ///
    /// Note 2: This event will not be emitted if a duplicate connection to the same node is
    ///         closed.
    Disconnected(N),

    /// Node closing
    ///
    /// This event is emitted whenever the node is shutting down. At this time, all connections
    /// are still active and can be used to send final messages.
    Closing,

    /// Node closed
    ///
    /// This event is emitted whenever the node has shut down. At this time, all connections
    /// have been closed and no final messages can be sent.
    Closed
}


/// A guard that closes the node when dropped.
///
/// Implementation details require the node to exist in multiple reference counted copies.
/// Therefore this wrapper is needed to implement the drop behavior.
/// Except for this drop behavior this struct can be used like the node struct that it encapsulates.
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
    node_id: N,
    events: Queue<Event<M, N, I>>,
    sockets: Mutex<Vec<(Arc<TcpListener>, JoinHandle<Result<(), Error<N>>>)>>,
    connections: RwLock<HashMap<N, Connection<M, N, I>>>,
    closed: RwLock<bool>,
    connection_timeout: Mutex<Duration>,
    stats_halflife_time: Mutex<Duration>,
    init_message: Mutex<I>,
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
///
/// There are several different constructors:
///
/// * [`new(...)`](#method.new) creates a new node with a given node id and initialization message.
/// * [`without_init(...)`](#method.without_init) creates a new node with a given node id but
///   without initialization message.
/// * [`with_random_id(...)`](#method.with_random_id) creates a new node with a random node id and
///   given initialization message.
/// * [`create_default()`](#method.create_default) creates a new node with a random node id and
///   without initialization message.
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
    /// The parameter `node_id` must be a unique identifier for this node and `init_message` is
    /// the first message sent automatically to other nodes when connected.
    /// The result of this call is a guard that closes the node if dropped.
    pub fn new(node_id: N, init_message: I) -> CloseGuard<M, N, I> {
        CloseGuard(Node(Arc::new(NodeInner{
            node_id: node_id,
            events: Queue::new(),
            sockets: Mutex::new(Vec::new()),
            connections: RwLock::new(HashMap::new()),
            closed: RwLock::new(false),
            connection_timeout: Mutex::new(Duration::from_secs(60)),
            stats_halflife_time: Mutex::new(Duration::from_secs(60)),
            init_message: Mutex::new(init_message),
        })))
    }

    /// Set the connection timeout
    ///
    /// Whenever a connection is idle longer than this timeout, it will be closed.
    /// The default value is 60 seconds.
    ///
    /// Note: Changes to this value only affect connections created after the change.
    pub fn set_connection_timeout(&self, dur: Duration) {
        *self.connection_timeout.lock().expect("Lock poisoned") = dur;
    }

    /// The connection timeout
    pub fn connection_timeout(&self) -> Duration {
        *self.connection_timeout.lock().expect("Lock poisoned")
    }

    /// Set the statistics half-life time
    ///
    /// This value is used as a parameter for the rolling average statistics of sending and
    /// receiving rates. Shorter durations react faster to changes and longer durations result in
    /// smoother behavior. The default value is 60 seconds.
    ///
    /// Note: Changes to this value only affect connections created after the change.
    pub fn set_stats_halflife_time(&self, dur: Duration) {
        *self.stats_halflife_time.lock().expect("Lock poisoned") = dur;
    }

    /// The statistics half-life time
    pub fn stats_halflife_time(&self) -> Duration {
        *self.stats_halflife_time.lock().expect("Lock poisoned")
    }

    /// Set the initialization message
    ///
    /// This message is sent as a first message on all new connections and must identify the node.
    pub fn set_init_message(&self, init: I) {
        *self.init_message.lock().expect("Lock poisoned") = init;
    }

    /// The initialization message
    pub fn init_message(&self) -> I {
        self.init_message.lock().expect("Lock poisoned").clone()
    }

    /// Listen on the specified address
    ///
    /// This method will open a new node listening to the given address. A dedicated thread will
    /// be started to handle incoming connections. The node can listen on multiple addresses.
    /// The result of this method, if successful, is the actual address used.
    ///
    /// Note: A port of `0` has the special meaning of taking a random free port. If the host part
    ///       of the address is `"0.0.0.0"` or `"[::0]"` the socket will be opened listening on
    ///       all available IPv4 or IPv6 addresses.
    pub fn listen<A: ToSocketAddrs>(&self, addr: A) -> Result<SocketAddr, Error<N>> {
        if *self.closed.read().expect("Lock poisoned") {
            return Err(Error::AlreadyClosed);
        }
        let mut nodes = self.sockets.lock().expect("Lock poisoned");
        let node: Arc<TcpListener> = Arc::new(try!(TcpListener::bind(addr).map_err(|err| Error::OpenError(err))));
        let cloned_self = self.clone();
        let cloned_node = node.clone();
        let used_addr = node.local_addr().expect("Failed to get local address");
        let join = thread::spawn(move || cloned_self.run_node(cloned_node));
        nodes.push((node, join));
        Ok(used_addr)
    }

    /// Listen on the addresses `"0.0.0.0:0"` and `"[::0]:0"`
    ///
    /// This will open sockets for IPv4 and IPv6 (see
    /// [`listen(...)`](struct.Node.html#method.listen) for details).
    ///
    /// Note: This method will always open two new sockets, regardless of whether sockets are
    ///       already open.
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

    /// Receive an event
    ///
    /// This method returns the next event received. If events are queued, the oldest queued event
    /// will be returned. Otherwise, the call will block until an event is received.
    ///
    /// If the node has been closed, `Event::Closed` will be returned immediately.
    pub fn receive(&self) -> Event<M, N, I> {
        match self.events.get() {
            Some(evt) => evt,
            None => Event::Closed
        }
    }

    /// Receive an event with timeout
    ///
    /// This method returns the next event received. If events are queued, the oldest queued event
    /// will be returned. Otherwise, the call will block until an event is received or the given
    /// timeout is reached (then `None` is returned).
    ///
    /// If the node has been closed, `Event::Closed` will be returned immediately.
    pub fn receive_timeout(&self, timeout: Duration) -> Option<Event<M, N, I>> {
        match self.events.get_timeout(timeout) {
            Some(Some(evt)) => Some(evt),
            Some(None) => Some(Event::Closed),
            None => None
        }
    }

    /// The id of this node
    pub fn node_id(&self) -> N {
        self.node_id.clone()
    }

    fn handle_message(&self, src: N, msg: M) {
        self.events.put(Event::Message(src, msg));
    }

    fn add_connection(&self, con: Connection<M, N, I>) {
        let id = con.node_id().clone();
        self.connections.write().expect("Lock poisoned").insert(id.clone(), con);
        self.events.put(Event::Connected(id));
    }

    fn del_connection(&self, id: &N) {
        self.connections.write().expect("Lock poisoned").remove(id);
        self.events.put(Event::Disconnected(id.clone()));
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

    fn run_node(&self, socket: Arc<TcpListener>) -> Result<(), Error<N>> {
        loop {
            let (sock, _) = try!(socket.accept().map_err(|e| Error::ConnectionError(e)));
            let req = try!(ConnectionRequest::new(self.clone(), sock));
            self.events.put(Event::ConnectionRequest(req));
        }
    }

    /// Send a message
    ///
    /// This method sends a message to the given destination. The message must be encodable using
    /// `serde` and the node must be connected to the destination.
    /// It is possible to send messages to the node itself by using its address.
    /// If no connection to the destination exists, an error is returned.
    pub fn send(&self, dst: &N, msg: &M) -> Result<(), Error<N>> {
        if dst == &self.node_id {
            self.handle_message(dst.clone(), msg.clone());
            return Ok(());
        }
        match self.get_connection(dst) {
            Some(con) => con.send(msg),
            None => Err(Error::NotConnected(dst.clone()))
        }
    }

    /// Open a connection and return the request
    ///
    /// This method opens a connection, exchanges initialization messages with the peer and returns
    /// a connection request. This request object can then be used to inspect the node id and the
    /// initialization message and then decide whether to `accept()` or `reject()` the connection.
    /// This method blocks until the underlying connection has been established and the
    /// initialization messages have been exchanged.
    ///
    /// For more details see [`connect(...)`](#method.connect).
    pub fn connect_request<A: ToSocketAddrs>(&self, addr: A) -> Result<ConnectionRequest<M, N, I>, Error<N>> {
        if *self.closed.read().expect("Lock poisoned") {
            return Err(Error::AlreadyClosed);
        }
        let sock = try!(TcpStream::connect(addr).map_err(|err| Error::ConnectionError(err)));
        Ok(try!(ConnectionRequest::new(self.clone(), sock)))
    }


    /// Open a connection and accept it automatically
    ///
    /// This method opens a connection, exchanges initialization messages with the peer,
    /// automatically accepts the peer and returns its node_id.
    /// This method blocks until the underlying connection has been established and the
    /// initialization messages have been exchanged.
    ///
    /// Note: This connection will automatically be closed when it becomes idle for longer than the
    ///       [specified timeout](#method.set_connection_timeout).
    ///
    /// Note 2: It is possible to connect the node to itself. However, messages will just be
    ///         short-circuited and the connection will not be used and closed after the timeout.
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<N, Error<N>> {
        let req = try!(self.connect_request(addr));
        let id = req.node_id().clone();
        req.accept();
        Ok(id)
    }

    fn shutdown_socket(&self, socket: &TcpListener) -> Result<(), Error<N>> {
        //TODO: Remove this workaround once a proper API is available
        let socket = unsafe { mem::transmute::<&TcpListener, &TcpStream>(socket) };
        socket.shutdown(Shutdown::Both).map_err(|e| Error::CloseError(e))
    }

    fn accept_connection(&self, con: Connection<M, N, I>) {
        self.add_connection(con.clone());
        thread::spawn(move || con.run());
    }

    fn close(&self) -> Result<(), Error<N>> {
        self.events.put(Event::Closing);
        *self.closed.write().expect("Lock poisoned") = true;
        let mut sockets = self.sockets.lock().expect("Lock poisoned");
        while let Some((s, j)) = sockets.pop() {
            try!(self.shutdown_socket(&s));
            j.join().expect("Failed to join").ok();
        }
        for c in self.get_connections() {
            let _ = c.close();
        }
        self.events.put(Event::Closed);
        self.events.close();
        Ok(())
    }
}

impl<M: Message, N: NodeId, I: InitMessage> Node<M, N, I> where N: Rand {
    /// Create a new node with a random id
    pub fn with_random_id(init: I) -> CloseGuard<M, N, I> {
        Node::new(rand::random::<N>(), init)
    }
}

impl<M: Message, N: NodeId> Node<M, N, ()> {
    /// Create a new node with a random id
    pub fn without_init(node_id: N) -> CloseGuard<M, N, ()> {
        Node::new(node_id, ())
    }
}

impl<M: Message, N: NodeId> Node<M, N, ()> where N: Rand {
    /// Create a new node with a random id and without init message
    pub fn create_default() -> CloseGuard<M, N, ()> {
        Node::new(rand::random::<N>(), ())
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


/// The request to establish a connection
///
/// This request is created for all new connections before they can be registered and used for
/// message exchange. The request contains the initialization message that has been received from
/// the remote side.
///
/// To accept the request, the method `accept()` has to be called.
///
/// To reject the connection request, the object can be dropped or the method `reject()` can be
/// called.
pub struct ConnectionRequest<M: Message, N: NodeId, I: InitMessage> {
    node: Node<M, N, I>,
    socket: TcpStream,
    init: I,
    node_id: N,
}

impl<M: Message, N: NodeId, I: InitMessage> PartialEq for ConnectionRequest<M, N, I> {
    fn eq(&self, other: &Self) -> bool {
        self.node_id == other.node_id
        && self.socket.peer_addr().unwrap() == other.socket.peer_addr().unwrap()
        && self.socket.local_addr().unwrap() == other.socket.local_addr().unwrap()
    }
}

impl<M: Message, N: NodeId, I: InitMessage> fmt::Debug for ConnectionRequest<M, N, I> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "ConnectionRequest(from: {:?}, node_id: {:?}, init: {:?})", self.socket.peer_addr().unwrap(), self.node_id, self.init)
    }
}

impl<M: Message, N: NodeId, I: InitMessage> ConnectionRequest<M, N, I> {
    fn new(node: Node<M, N, I>, mut socket: TcpStream) -> Result<Self, Error<N>> {
        try!(socket.set_nodelay(true).map_err(|err| Error::ConnectionError(err)));
        try!(socket.set_read_timeout(Some(node.connection_timeout())).map_err(|err| Error::ConnectionError(err)));
        {
            let mut writer = rmp_serde::Serializer::new(&mut socket);
            try!((node.init_message(), node.node_id()).serialize(&mut writer).map_err(|_| Error::SendError));
        }
        let (init, node_id) = {
            let mut reader = rmp_serde::Deserializer::new(&socket);
            try!(Deserialize::deserialize(&mut reader).map_err(|_| Error::ReadError))
        };
        Ok(ConnectionRequest{node: node, socket: socket, init: init, node_id: node_id})
    }

    /// The initialization message received from the remote side.
    pub fn init_message(&self) -> &I {
        &self.init
    }

    /// The node id received from the remote side.
    pub fn node_id(&self) -> &N {
        &self.node_id
    }

    /// Accept the connection
    ///
    /// When this method is called, the connection is registered and is usable for message exchange
    /// afterwards.
    pub fn accept(self) {
        let con = Connection::new(self.node.clone(), self.socket, self.node_id);
        self.node.accept_connection(con);
    }

    /// Reject the connection
    pub fn reject(self) {
        drop(self.socket);
    }
}


pub struct ConnectionInner<M: Message, N: NodeId, I: InitMessage> {
    node: Node<M, N, I>,
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
    fn new(node: Node<M, N, I>, socket: TcpStream, node_id: N) -> Self {
        let writer = StatWriter::new(socket.try_clone().expect("Failed to clone socket"), node.stats_halflife_time());
        let writer_stats = writer.stats();
        let input = StatReader::new(socket.try_clone().expect("Failed to clone socket"), node.stats_halflife_time());
        let reader_stats = input.stats();
        let reader = rmp_serde::Deserializer::new(input);
        Connection(Arc::new(ConnectionInner{
            node: node,
            writer: Mutex::new(writer),
            writer_stats: writer_stats,
            reader: Mutex::new(reader),
            reader_stats: reader_stats,
            socket: Mutex::new(socket),
            node_id: node_id
        }))
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
        self.node.del_connection(&self.node_id);
        res
    }

    fn run_inner(&self) -> Result<(), Error<N>> {
        let mut reader = self.reader.lock().expect("Lock poisoned");
        loop {
            let msg = try!(M::deserialize(&mut reader as &mut rmp_serde::Deserializer<StatReader<TcpStream>>).map_err(|_| Error::ReadError));
            self.node.handle_message(self.node_id.clone(), msg);
        }
    }

    fn close(&self) -> Result<(), Error<N>> {
        Ok(try!(self.socket.lock().expect("Lock poisoned").shutdown(Shutdown::Both).map_err(|err| Error::CloseError(err))))
    }
}
