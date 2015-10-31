use super::*;

use test::Bencher;
use std::time::Duration;
use std::sync::{Condvar, Mutex, Arc};
use std::collections::VecDeque;


struct BouncerCallbackInner {
    id: u64
}

#[derive(Clone)]
struct BouncerCallback(Arc<BouncerCallbackInner>);

impl BouncerCallback {
    fn new(id: u64) -> Self {
        BouncerCallback(Arc::new(BouncerCallbackInner{id: id}))
    }
}

impl Callback<u64, u64, u64> for BouncerCallback {
    fn node_id(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0.id
    }

    fn create_init_msg(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0.id
    }

    fn node_id_from_init_msg(&self, _node: &Node<u64, u64, u64>, id: &u64) -> u64 {
        *id
    }

    fn connection_timeout(&self, _node: &Node<u64, u64, u64>) -> Duration {
        Duration::from_secs(1)
    }

    fn on_connected(&self, _node: &Node<u64, u64, u64>, _id: &u64) {
    }

    fn on_disconnected(&self, _node: &Node<u64, u64, u64>, _id: &u64) {
    }

    fn handle_message(&self, node: &Node<u64, u64, u64>, src: &u64, msg: u64) {
        node.send(*src, &msg).expect("Failed to send");
    }
}

#[derive(Debug, PartialEq)]
enum Event {
    Msg(u64, u64),
    Connected(u64),
    Disconnected(u64),
}

struct DummyCallbackInner {
    msgs: Mutex<VecDeque<Event>>,
    waiter: Condvar,
    id: u64
}

#[derive(Clone)]
struct DummyCallback(Arc<DummyCallbackInner>);

impl DummyCallback {
    fn new(id: u64) -> Self {
        DummyCallback(Arc::new(DummyCallbackInner{msgs: Mutex::new(VecDeque::new()), waiter: Condvar::new(), id: id}))
    }

    fn recv(&self) -> Event {
        let mut lock = self.0.msgs.lock().expect("Lock poisoned");
        if lock.len() > 0 {
            return lock.pop_front().unwrap();
        }
        let mut lock = self.0.waiter.wait(lock).expect("Lock poisoned");
        lock.pop_front().unwrap()
    }
}

impl Callback<u64, u64, u64> for DummyCallback {
    fn node_id(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0.id
    }

    fn create_init_msg(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.0.id
    }

    fn node_id_from_init_msg(&self, _node: &Node<u64, u64, u64>, id: &u64) -> u64 {
        *id
    }

    fn connection_timeout(&self, _node: &Node<u64, u64, u64>) -> Duration {
        Duration::from_secs(1)
    }

    fn on_connected(&self, _node: &Node<u64, u64, u64>, id: &u64) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(Event::Connected(*id));
        self.0.waiter.notify_all();
    }

    fn on_disconnected(&self, _node: &Node<u64, u64, u64>, id: &u64) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(Event::Disconnected(*id));
        self.0.waiter.notify_all();
    }

    fn handle_message(&self, _node: &Node<u64, u64, u64>, src: &u64, msg: u64) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(Event::Msg(*src, msg));
        self.0.waiter.notify_all();
    }
}


#[test]
fn node_create() {
    let callback = DummyCallback::new(0);
    let node = Node::new(Box::new(callback));
    assert!(node.close().is_ok());
}

#[test]
fn node_open() {
    let callback = DummyCallback::new(0);
    let node = Node::new(Box::new(callback));
    assert!(node.open("localhost:0").is_ok());
    let addrs = node.addresses();
    assert_eq!(addrs.len(), 1);
    assert!(node.close().is_ok());
}

#[bench]
fn node_send_self(b: &mut Bencher) {
    let callback = DummyCallback::new(0);
    let node = Node::new(Box::new(callback.clone()));
    assert!(node.open("localhost:0").is_ok());
    b.iter(|| {
        assert!(node.send(0, &42).is_ok());
        assert_eq!(callback.recv(), Event::Msg(0, 42))
    });
    assert!(node.close().is_ok());
}

#[test]
fn node_connect() {
    let server = Node::new(Box::new(BouncerCallback::new(0)));
    assert!(server.open("localhost:0").is_ok());
    let client_callback = DummyCallback::new(1);
    let client = Node::new(Box::new(client_callback.clone()));
    assert!(client.open("localhost:0").is_ok());
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert_eq!(client_callback.recv(), Event::Connected(0));
    assert!(client.close().is_ok());
    assert!(server.close().is_ok());
}

#[bench]
fn node_send_remote(b: &mut Bencher) {
    let server = Node::new(Box::new(BouncerCallback::new(0)));
    assert!(server.open("localhost:0").is_ok());
    let client_callback = DummyCallback::new(1);
    let client = Node::new(Box::new(client_callback.clone()));
    assert!(client.open("localhost:0").is_ok());
    assert!(client.connect(server.addresses()[0]).is_ok());
    assert_eq!(client_callback.recv(), Event::Connected(0));
    b.iter(|| {
        assert!(client.send(0, &42).is_ok());
        assert_eq!(client_callback.recv(), Event::Msg(0, 42))
    });
    assert!(client.close().is_ok());
    assert!(server.close().is_ok());
}
