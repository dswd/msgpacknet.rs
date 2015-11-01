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
//! * [`InitMsg`](trait.InitMsg.html) - The first message exchanged on all connections uses the
//! `InitMsg` trait instead of `Message`. This first message must include the
//! `NodeId` of the remote node to identify it.
//!
//! # Low-level protocol
//! When establishing a new connection (either incoming or outgoing), first an `InitMsg` is sent to
//! the remote node and then the initialization message is read from that node. After identifying
//! the remote node the connection is either rejected and closed or accepted and added to the
//! connection registry.
//!
//! When established, the connection can be used to send multiple messages of the `Message` type.
//!
//! When idle for a certain timeout or when closing the node by dropping its JoinGuard, the
//! connection is closed.

//#![cfg_attr(test, feature(test))]
#![cfg_attr(not(build="release"), feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
extern crate time;
#[cfg(not(build="release"))] extern crate test;

mod stats;
mod socket;
mod simple;

#[doc(hidden)]
#[cfg(not(build="release"))]
pub mod tests;

pub use simple::{SimpleCallback, SimpleCallbackEvent};
pub use socket::{NodeStats, ConnectionStats, Node, Message, InitMsg, Callback, NodeId, Error};
