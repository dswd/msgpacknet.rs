#![cfg_attr(test, feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
extern crate time;
extern crate rand;
#[cfg(test)] extern crate test;

#[cfg(test)] mod tests;

use serde::Deserialize;
use net2::TcpStreamExt;

use std::net::{TcpListener, TcpStream, ToSocketAddrs, Shutdown};
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

pub trait Message: serde::Serialize + serde::Deserialize + Send + Sync + Debug + Clone + 'static {}
pub trait NodeId: serde::Serialize + serde::Deserialize + Send + Sync + Debug + Clone + Eq + Hash + 'static {}
pub trait InitMsg: serde::Serialize + serde::Deserialize + Send + Sync + Debug + Clone + 'static {}

pub enum Error<N: NodeId> {
    BindError(IoError),
    AcceptError(IoError),
    SocketOptionError(IoError),
    Serialize(rmp_serde::encode::Error),
    Deserialize(rmp_serde::decode::Error),
    UnreachableDestination(N),
    ConnectError(IoError),
    CloseServer(IoError),
    CloseConnection(IoError),
}

pub trait Callback<M: Message, N: NodeId, I: InitMsg>: Send + Sync + 'static {
    fn node_id(&self) -> N;
    fn create_init_msg(&self) -> I;
    fn connection_timeout(&self) -> Duration;
    fn node_id_from_init_msg(&self, &I) -> N;
    fn handle_message(&self, &N, M);
    fn on_connected(&self, &N);
    fn on_disconnected(&self, &N);
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
    pub fn new(callback: Box<Callback<M, N, I>>) -> Self {
        Node(Arc::new(NodeInner{
            callback: callback,
            sockets: Mutex::new(Vec::new()),
            connections: RwLock::new(HashMap::new()),
        }))
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

    fn node_id(&self) -> N {
        self.callback.node_id()
    }

    fn create_init_msg(&self) -> I {
        self.callback.create_init_msg()
    }

    fn node_id_from_init_msg(&self, init: &I) -> N {
        self.callback.node_id_from_init_msg(&init)
    }

    fn connection_timeout(&self) -> Duration {
        self.callback.connection_timeout()
    }

    fn handle_message(&self, src: &N, msg: M) {
        self.callback.handle_message(&src, msg)
    }

    fn add_connection(&self, con: Connection<M, N, I>) {
        let id = con.node_id().clone();
        self.connections.write().expect("Lock poisoned").insert(id.clone(), con);
        self.callback.on_connected(&id);
    }

    fn del_connection(&self, id: &N) {
        self.connections.write().expect("Lock poisoned").remove(id);
        self.callback.on_connected(id);
    }

    fn get_connection(&self, id: &N) -> Option<Connection<M, N, I>> {
        self.connections.read().expect("Lock poisoned").get(id).map(|v| v.clone())
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
        let con = match self.get_connection(&dst) {
            Some(con) => con,
            None => return Err(Error::UnreachableDestination(dst))
        };
        con.send(msg)
    }

    #[inline]
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<(), Error<N>> {
        let sock = try!(TcpStream::connect(addr).map_err(|err| Error::ConnectError(err)));
        let con = try!(Connection::new(self.clone(), sock));
        self.callback.on_connected(con.node_id());
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
            try!(init.serialize(&mut writer).map_err(|e| Error::Serialize(e)));
        }
        let init = {
            let mut reader = rmp_serde::Deserializer::new(&socket);
            try!(I::deserialize(&mut reader).map_err(|e| Error::Deserialize(e)))
        };
        let node_id = server.node_id_from_init_msg(&init);
        Ok(Connection(Arc::new(ConnectionInner{server: server, socket: RwLock::new(socket), node_id: node_id})))
    }

    fn node_id(&self) -> &N {
        &self.node_id
    }

    fn send(&self, msg: &M) -> Result<(), Error<N>> {
        let mut lock = self.socket.write().expect("Lock poisoned");
        let mut bufwriter = BufWriter::new(&mut lock as &mut TcpStream);
        let mut writer = rmp_serde::Serializer::new(&mut bufwriter);
        msg.serialize(&mut writer).map_err(|e| Error::Serialize(e))
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
            let msg = try!(M::deserialize(&mut reader).map_err(|e| Error::Deserialize(e)));
            self.server.handle_message(&self.node_id, msg);
        }
    }

    fn close(&self) -> Result<(), Error<N>> {
        Ok(try!(self.socket.read().expect("Lock poisoned").shutdown(Shutdown::Both).map_err(|err| Error::CloseConnection(err))))
    }
}
