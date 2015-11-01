#![cfg_attr(test, feature(test))]
extern crate serde;
extern crate rmp_serde;
extern crate net2;
#[cfg(test)] extern crate test;

mod socket;
#[cfg(test)] mod tests;

pub use socket::{Node, Message, InitMsg, Callback, NodeId, Error};
