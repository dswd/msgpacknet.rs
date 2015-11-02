use super::*;

use test::Bencher;
use std::time::Duration;

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

#[bench]
fn node_send_self(b: &mut Bencher) {
    let node = Node::new(0, 0);
    assert!(node.listen("localhost:0").is_ok());
    b.iter(|| {
        assert!(node.send(&0, &42).is_ok());
        assert_eq!(node.receive_timeout(Duration::from_secs(1)).unwrap(), Event::Message(0, 42))
    });
    assert!(!node.is_connected(&0));
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
    let evt = server.receive_timeout(Duration::from_secs(1));
    assert!(evt.is_some());
    if let Event::ConnectionRequest(req) = evt.unwrap() {
        req.accept();
    } else {
        assert!(false);
    }
    assert!(server.is_connected(&1));
    assert_eq!(client.receive_timeout(Duration::from_secs(1)).unwrap(), Event::Connected(0));
}

#[bench]
fn node_send_remote(b: &mut Bencher) {
    let server = Node::<u64, u64, u64>::new(0, 0);
    assert!(server.listen("localhost:0").is_ok());
    let client = Node::new(1, 1);
    assert!(client.listen("localhost:0").is_ok());
    assert!(client.connect(server.addresses()[0]).is_ok());
    if let Event::ConnectionRequest(req) = server.receive_timeout(Duration::from_secs(1)).unwrap() {
        req.accept();
    } else {
        assert!(false);
    }
    assert_eq!(client.receive_timeout(Duration::from_secs(1)).unwrap(), Event::Connected(0));
    assert_eq!(server.receive_timeout(Duration::from_secs(1)).unwrap(), Event::Connected(1));
    b.iter(|| {
        assert!(client.send(&0, &42).is_ok());
        assert_eq!(server.receive_timeout(Duration::from_secs(1)).unwrap(), Event::Message(1, 42))
    });
}
