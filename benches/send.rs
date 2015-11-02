#![feature(test)]
extern crate test;
extern crate msgpacknet;

use msgpacknet::*;
use test::Bencher;

#[bench]
fn node_send_self(b: &mut Bencher) {
    let node = Node::new(0, 0);
    assert!(node.listen("localhost:0").is_ok());
    b.iter(|| {
        assert!(node.send(&0, &42).is_ok());
        assert_eq!(node.receive(), Event::Message(0, 42))
    });
    assert!(!node.is_connected(&0));
}

#[bench]
fn node_send_remote(b: &mut Bencher) {
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
    b.iter(|| {
        assert!(client.send(&0, &42).is_ok());
        assert_eq!(server.receive(), Event::Message(1, 42))
    });
}
