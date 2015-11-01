use super::*;

use test::Bencher;

type DummyCallback = SimpleCallback<u64, u64>;

#[derive(Clone)]
pub struct BouncerCallback(u64);

impl BouncerCallback {
    pub fn new(id: u64) -> Self {
        BouncerCallback(id)
    }
}

impl Callback<u64, u64, u64> for BouncerCallback {
    fn node_id(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0
    }

    fn create_init_msg(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0
    }

    fn handle_init_msg(&self, _node: &Node<u64, u64, u64>, id: &u64) -> Option<u64> {
        Some(*id)
    }

    fn handle_message(&self, node: &Node<u64, u64, u64>, src: &u64, msg: u64) {
        node.send(*src, &msg).expect("Failed to send");
    }
}


#[test]
fn node_create() {
    let callback = DummyCallback::new(0);
    let _ = Node::new(Box::new(callback));
}

#[test]
fn node_open() {
    let callback = DummyCallback::new(0);
    let node = Node::new(Box::new(callback));
    assert!(node.listen("localhost:0").is_ok());
    let addrs = node.addresses();
    assert_eq!(addrs.len(), 1);
}

#[bench]
fn node_send_self(b: &mut Bencher) {
    let callback = DummyCallback::new(0);
    let node = Node::new(Box::new(callback.clone()));
    assert!(node.listen("localhost:0").is_ok());
    b.iter(|| {
        assert!(node.send(0, &42).is_ok());
        assert_eq!(callback.receive(), SimpleCallbackEvent::Msg(0, 42))
    });
    assert!(!node.is_connected(&0));
}

#[test]
fn node_connect() {
    let server = Node::new(Box::new(BouncerCallback::new(0)));
    assert!(server.listen("localhost:0").is_ok());
    let client_callback = DummyCallback::new(1);
    let client = Node::new(Box::new(client_callback.clone()));
    assert!(client.listen("localhost:0").is_ok());
    assert!(!client.is_connected(&0));
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert!(client.is_connected(&0));
    assert_eq!(client_callback.receive(), SimpleCallbackEvent::Connected(0));
}

#[bench]
fn node_send_remote(b: &mut Bencher) {
    let server = Node::new(Box::new(BouncerCallback::new(0)));
    assert!(server.listen("localhost:0").is_ok());
    let client_callback = DummyCallback::new(1);
    let client = Node::new(Box::new(client_callback.clone()));
    assert!(client.listen("localhost:0").is_ok());
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert_eq!(client_callback.receive(), SimpleCallbackEvent::Connected(0));
    b.iter(|| {
        assert!(client.send(0, &42).is_ok());
        assert_eq!(client_callback.receive(), SimpleCallbackEvent::Msg(0, 42))
    });
}
