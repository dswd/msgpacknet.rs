use super::*;

use std::sync::{Condvar, Mutex, Arc};
use std::collections::VecDeque;


/// The enum used for events
#[derive(Debug, PartialEq)]
pub enum SimpleCallbackEvent<M, N> {
    /// A received message
    Msg(N, M),

    /// A new connection
    Connected(N),

    /// A lost connection
    Disconnected(N),
}

pub struct SimpleCallbackInner<M, N> {
    msgs: Mutex<VecDeque<SimpleCallbackEvent<M, N>>>,
    waiter: Condvar,
    id: N,
}

/// A simple implementation of the [`Callback`](trait.Callback.html) trait
///
/// This implementation has the following properties:
///
/// * It uses the node id as an initialization message.
/// * It accepts all incoming connections.
/// * It stores all events in a queue of [`SimpleCallbackEvent`](enum.SimpleCallbackEvent.html).
#[derive(Clone)]
pub struct SimpleCallback<M, N>(Arc<SimpleCallbackInner<M, N>>);

impl<M, N> SimpleCallback<M, N> {
    /// Creates a new callback
    ///
    /// The parameter `id` is used as this nodes id and should be unique.
    pub fn new(id: N) -> Self {
        SimpleCallback(Arc::new(SimpleCallbackInner{msgs: Mutex::new(VecDeque::new()), waiter: Condvar::new(), id: id}))
    }

    /// Retrieves an event from the queue
    ///
    /// This method takes the oldest event from the queue and returns it. If the queue is empty,
    /// this call blocks until an event is received.
    pub fn recv(&self) -> SimpleCallbackEvent<M, N> {
        let mut lock = self.0.msgs.lock().expect("Lock poisoned");
        if lock.len() > 0 {
            return lock.pop_front().unwrap();
        }
        let mut lock = self.0.waiter.wait(lock).expect("Lock poisoned");
        lock.pop_front().unwrap()
    }
}

impl<M: Message, N: NodeId> Callback<M, N, N> for SimpleCallback<M, N> {
    fn node_id(&self, _node: &Node<M, N, N>) -> N {
        self.0.id.clone()
    }

    fn create_init_msg(&self, _node: &Node<M, N, N>) -> N {
        self.0.id.clone()
    }

    fn handle_init_msg(&self, _node: &Node<M, N, N>, id: &N) -> Option<N> {
        Some(id.clone())
    }

    fn on_connected(&self, _node: &Node<M, N, N>, id: &N) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(SimpleCallbackEvent::Connected(id.clone()));
        self.0.waiter.notify_all();
    }

    fn on_disconnected(&self, _node: &Node<M, N, N>, id: &N) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(SimpleCallbackEvent::Disconnected(id.clone()));
        self.0.waiter.notify_all();
    }

    fn handle_message(&self, _node: &Node<M, N, N>, src: &N, msg: M) {
        self.0.msgs.lock().expect("Lock poisoned").push_back(SimpleCallbackEvent::Msg(src.clone(), msg));
        self.0.waiter.notify_all();
    }
}
