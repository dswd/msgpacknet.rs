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
