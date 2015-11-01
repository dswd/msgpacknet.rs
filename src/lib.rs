//! A networking layer based on MessagePack messages.
//!
//! This crate provides an abstraction layer above TCP that uses
//! [MessagePack](http://www.msgpack.org) encoded messages instead of pure byte streams.
//! It also abstracts from addresses and connections and instead uses node identifiers to
//! distinguish and address nodes.
//!
//! The main struct of this crate is [`Node`](struct.Node.html) which can be parametrized:
//!
//! * [`Message`](trait.Message.html) - The nodes using this crate communicate via messages that
//! conform to the `Message` trait. These messages are serialized using `serde` to the
//! efficient MessagePack format.
//!
//! * [`NodeId`](trait.NodeId.html) - This crate identifies and distinguishes nodes based on their
//! `NodeId`. It is required that all communicating nodes have different ids.
//! Therefore the node ids should be unique. This can be achieved by using the ip address and port
//! number of the main listening socket. Alternatively, a large random number can be used (128 bit
//! should be enough to expect uniqueness).
//!
//! * [`InitMessage`](trait.InitMessage.html) - The first message exchanged on all connections uses
//! the `InitMessage` trait instead of `Message`. This first message must include the
//! `NodeId` of the remote node to identify it.
//!
//! # Low-level protocol
//! When establishing a new connection (either incoming or outgoing), first an `InitMessage` is
//! sent to the remote node and then the initialization message is read from that node.
//! After identifying the remote node the connection is either rejected and closed or accepted and
//! added to the connection registry.
//!
//! When established, the connection can be used to send multiple messages of the `Message` type.
//!
//! When idle for a certain timeout or when closing the node by dropping its
//! [`CloseGuard`](struct.CloseGuard.html), the connection is closed.
//!
//! # Examples
//! To use this crate, first a callback conforming to the [`Callback`](trait.Callback.html) trait
//! has to be created. The [`SimpleCallback`](struct.SimpleCallback.html) struct provides a simple
//! implementation. The callback has to be created with a node id which should be unique.
//!
//! ```
//! # use msgpacknet::*;
//! let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.send(node.node_id(), &()).expect("Failed to send");
//! ```
//!
//! Afterwards, the node can be created with a boxed copy of the callback as parameter.
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.send(node.node_id(), &()).expect("Failed to send");
//! ```
//!
//! Then, sockets can be opened for listening and accepting connections.
//! The method `listen_defaults()` can be used to listen on free ports on IPv4 and IPv6.
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! node.listen_defaults().expect("Failed to bind");
//! # node.send(node.node_id(), &()).expect("Failed to send");
//! ```
//!
//! The actual address can be obtained using `addresses()`.
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.listen_defaults().expect("Failed to bind");
//! println!("Addresses: {:?}", node.addresses());
//! # node.send(node.node_id(), &()).expect("Failed to send");
//! ```
//!
//! Connections to other nodes can be established via `connect`. The result of the call is the id
//! of that peer.
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.listen_defaults().expect("Failed to bind");
//! # let addr = node.addresses()[0];
//! let peer_id = node.connect(addr).expect("Failed to connect");
//! # node.send(peer_id, &()).expect("Failed to send");
//! ```
//!
//! Then, messages can be sent via `node.send`...
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.listen_defaults().expect("Failed to bind");
//! # let peer_id = node.node_id();
//! # let msg = ();
//! node.send(peer_id, &msg).expect("Failed to send");
//! let reply = callback.recv();
//! ```
//!
//! ...and received via `callback.recv`.
//!
//! ```
//! # use msgpacknet::*;
//! # let callback = SimpleCallback::with_random_id();
//! # let node = Node::new(Box::new(callback.clone()));
//! # let _dummy: u64 = node.node_id();
//! # node.listen_defaults().expect("Failed to bind");
//! # let peer_id = node.node_id();
//! # let msg = ();
//! # node.send(peer_id, &msg).expect("Failed to send");
//! let reply = callback.recv();
//! ```

//#![cfg_attr(test, feature(test))]
#![cfg_attr(test, feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
extern crate time;
extern crate rand;
#[cfg(test)] extern crate test;

mod stats;
mod socket;
mod simple;
#[cfg(test)] mod tests;

pub use simple::{SimpleCallback, SimpleCallbackEvent};
pub use socket::{CloseGuard, NodeStats, ConnectionStats, Node, Message, InitMessage, Callback, NodeId, Error};
