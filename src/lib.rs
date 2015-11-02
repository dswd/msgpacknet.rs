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
//! Therefore the node ids should be unique. This can be achieved by using the IP address and port
//! number of the main listening socket. Alternatively, a large random number can be used (128 bit
//! should be enough to expect uniqueness).
//!
//! * [`InitMessage`](trait.InitMessage.html) - The first message exchanged on all connections uses
//! the `InitMessage` trait instead of `Message`. This first message must include the
//! `NodeId` of the sender to identify it.
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
//! To use this crate, first a [`Node`](struct.Node.html) has to be created with a node id and an
//! initialization message that contains this id and which is sent to other nodes as first message.
//!
//! ```
//! use msgpacknet::*;
//! let node = Node::<String, (u64, u64)>::with_random_id();
//! ```
//!
//! Then, sockets can be opened for listening and accepting connections.
//! The method [`node.listen_defaults()`](struct.Node.html#method.listen_defaults) can be used to
//! listen on free ports on IPv4 and IPv6.
//!
//! ```
//! # use msgpacknet::*;
//! # let node = Node::<String, (u64, u64)>::with_random_id();
//! node.listen_defaults().expect("Failed to bind");
//! ```
//!
//! The actual address can be obtained using
//! [`node.addresses()`](struct.Node.html#method.addresses).
//!
//! ```
//! # use msgpacknet::*;
//! # let node = Node::<String, (u64, u64)>::with_random_id();
//! # node.listen_defaults().expect("Failed to bind");
//! println!("Addresses: {:?}", node.addresses());
//! let addr = node.addresses()[0];
//! ```
//!
//! Connections to other nodes can be established via
//! [`connect(...)`](struct.Node.html#method.connect). The result of the call is a
//! [`ConnectionRequest`](struct.ConnectionRequest.html) which can be used to accept the
//! connection and identify the remote side's node id.
//!
//! ```
//! # use msgpacknet::*;
//! # let node = Node::<String, (u64, u64)>::with_random_id();
//! # node.listen_defaults().expect("Failed to bind");
//! # let addr = node.addresses()[0];
//! let request = node.connect(addr).expect("Failed to connect");
//! let peer_id = request.init_message().clone();
//! request.accept(peer_id.clone());
//! ```
//!
//! Then, messages can be sent via [`node.send(...)`](struct.Node.html#method.send)...
//!
//! ```
//! # use msgpacknet::*;
//! # let node = Node::<String, (u64, u64)>::with_random_id();
//! # let peer_id = node.node_id();
//! let msg = "Hello world".to_owned();
//! node.send(&peer_id, &msg).expect("Failed to send");
//! ```
//!
//! ...and received via [`callback.receive()`](struct.SimpleCallback.html#method.receive).
//!
//! ```
//! # use msgpacknet::*;
//! # let node = Node::<(), (u64, u64)>::with_random_id();
//! # node.send(&node.node_id(), &()).expect("Failed to send");
//! let reply = node.receive();
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
mod queue;
mod socket;
#[cfg(test)] mod tests;

pub use socket::{Event, ConnectionRequest, CloseGuard, NodeStats, ConnectionStats, Node, Message, InitMessage, NodeId, Error};
