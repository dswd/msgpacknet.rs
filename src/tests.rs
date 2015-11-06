use super::*;
use super::stats::Stats;

use std::time::Duration;

#[test]
fn stats() {
    let mut stats = Stats::new(Duration::from_millis(100));
    assert_eq!(stats.total(), 0);
    assert_eq!(stats.rate(), 0.0);
    let idle1 = stats.idle_time();
    assert!(idle1 < Duration::from_millis(100));
    assert!(idle1 > Duration::from_millis(0));
    stats.update(100);
    assert_eq!(stats.total(), 100);
    let idle2 = stats.idle_time();
    assert!(idle2 < Duration::from_millis(100));
    assert!(idle2 > Duration::from_millis(0));
    assert!(idle1 != idle2);
    assert!(stats.rate() > 0.0);
}

#[test]
fn node_create() {
    let _ = Node::<u64, u64, u64>::new(0, 0);
}

#[test]
fn node_open() {
    let node = Node::<u64, u64, u64>::new(0, 0);
    assert!(node.listen("localhost:0").is_ok());
    let addrs = node.addresses();
    assert_eq!(addrs.len(), 1);
}

#[test]
fn node_connect() {
    let server = Node::<u64, u64, u64>::new(0, 0);
    assert!(server.listen("localhost:0").is_ok());
    let client = Node::<u64, u64, u64>::new(1, 1);
    assert!(client.listen("localhost:0").is_ok());
    assert!(!client.is_connected(&0));
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert!(client.is_connected(&0));
    let evt = server.receive();
    if let Event::ConnectionRequest(req) = evt {
        req.accept();
    } else {
        assert!(false);
    }
    assert!(server.is_connected(&1));
    assert_eq!(client.receive(), Event::Connected(0));
}

#[test]
fn node_send_self() {
    let node = Node::new(0, 0);
    assert!(node.listen("localhost:0").is_ok());
    assert!(node.send(&0, &42).is_ok());
    assert_eq!(node.receive(), Event::Message(0, 42));
    assert!(!node.is_connected(&0));
}

#[test]
fn node_send_remote() {
    let server = Node::<u64, u64, u64>::new(0, 0);
    assert!(server.listen("localhost:0").is_ok());
    let client = Node::new(1, 1);
    assert!(client.listen("localhost:0").is_ok());
    assert!(client.connect(server.addresses()[0]).is_ok());
    if let Event::ConnectionRequest(req) = server.receive() {
        req.accept();
    } else {
        assert!(false);
    }
    assert_eq!(client.receive(), Event::Connected(0));
    assert_eq!(server.receive(), Event::Connected(1));
    assert!(client.send(&0, &42).is_ok());
    assert_eq!(server.receive(), Event::Message(1, 42));
}

#[test]
fn node_lookup_connection() {
    let server = Node::<u64, u64, u64>::new(0, 0);
    assert!(server.listen("localhost:0").is_ok());
    let client = Node::<u64, u64, u64>::new(1, 1);
    assert!(client.listen("localhost:0").is_ok());
    assert_eq!(client.lookup_connection(server.addresses()[0]), None);
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert_eq!(client.lookup_connection(server.addresses()[0]), Some(server.node_id()));
}
