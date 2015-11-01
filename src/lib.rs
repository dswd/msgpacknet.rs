//! A networking layer based on MsgPack messages.
//!
//! This crate provides an abstraction layer above TCP that uses MsgPack encoded messages instead
//! of pure byte streams. It also abstracts from addresses and connections and instead uses node
//! identifiers to distinguish and address nodes.

#![cfg_attr(test, feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
extern crate time;
#[cfg(test)] extern crate test;

mod stats;
mod socket;
#[cfg(test)] mod tests;

pub use socket::{NodeStats, ConnectionStats, Node, Message, InitMsg, Callback, NodeId, Error};
