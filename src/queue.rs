use std::sync::{Arc, Condvar, Mutex};
use std::collections::VecDeque;
#[cfg(feature = "nightly")]
use std::time::Duration;

pub struct QueueInner<T> {
    msgs: Mutex<(VecDeque<T>, bool)>,
    waiter: Condvar,
}

#[derive(Clone)]
pub struct Queue<T>(Arc<QueueInner<T>>);

impl<T> Queue<T> {
    pub fn new() -> Self {
        Queue(Arc::new(QueueInner {
            msgs: Mutex::new((VecDeque::new(), true)),
            waiter: Condvar::new(),
        }))
    }

    pub fn get(&self) -> Option<T> {
        let mut lock = self.0.msgs.lock().expect("Lock poisoned");
        while lock.1 && lock.0.is_empty() {
            lock = self.0.waiter.wait(lock).expect("Lock poisoned");
        }
        debug_assert!(lock.1 || !lock.0.is_empty());
        lock.0.pop_front()
    }

    #[cfg(feature = "nightly")]
    pub fn get_timeout(&self, timeout: Duration) -> Option<Option<T>> {
        let mut lock = self.0.msgs.lock().expect("Lock poisoned");
        while lock.1 && lock.0.is_empty() {
            let (new_lock, result) = self.0
                                         .waiter
                                         .wait_timeout(lock, timeout)
                                         .expect("Lock poisoned");
            if result.timed_out() {
                return None;
            }
            lock = new_lock;
        }
        debug_assert!(lock.1 || !lock.0.is_empty());
        Some(lock.0.pop_front())
    }

    pub fn close(&self) {
        self.0.msgs.lock().expect("Lock poisoned").1 = false;
    }

    pub fn put(&self, item: T) {
        let mut lock = self.0.msgs.lock().expect("Lock poisoned");
        if !lock.1 {
            return;
        }
        lock.0.push_back(item);
        self.0.waiter.notify_all();
    }
}

impl<T> Iterator for Queue<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.get()
    }
}
