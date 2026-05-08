use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub enum SendError<T> {
    /// The receiver has been dropped.
    Disconnected(T),
    /// The channel is full (bounded channels only).
    Full(T),
}

#[derive(Debug, PartialEq)]
pub enum RecvError {
    Empty,
    Disconnected,
}

struct Inner<T> {
    queue: VecDeque<T>,
    capacity: Option<usize>, // None when unbounded
    sender_count: usize,
    receiver_alive: bool,
}

impl<T> Inner<T> {
    fn new(capacity: Option<usize>) -> Self {
        Self {
            queue: VecDeque::new(),
            capacity,
            sender_count: 1,
            receiver_alive: true,
        }
    }

    fn is_full(&self) -> bool {
        self.capacity.map_or(false, |cap| self.queue.len() >= cap)
    }
}

pub struct Sender<T> {
    inner: Rc<RefCell<Inner<T>>>,
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = self.inner.borrow_mut();
        if !inner.receiver_alive {
            return Err(SendError::Disconnected(value));
        }
        if inner.is_full() {
            return Err(SendError::Full(value));
        }
        inner.queue.push_back(value);
        Ok(())
    }

    pub fn is_disconnected(&self) -> bool {
        !self.inner.borrow().receiver_alive
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.borrow_mut().sender_count += 1;
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.inner.borrow_mut().sender_count -= 1;
    }
}

// ──────────────────────────────────────────────────────────
// Receiver
// ──────────────────────────────────────────────────────────

pub struct Receiver<T> {
    inner: Rc<RefCell<Inner<T>>>,
}

impl<T> Receiver<T> {
    pub fn try_recv(&self) -> Result<T, RecvError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(value) = inner.queue.pop_front() {
            return Ok(value);
        }
        if inner.sender_count == 0 {
            Err(RecvError::Disconnected)
        } else {
            Err(RecvError::Empty)
        }
    }

    pub fn drain(&self) -> Vec<T> {
        let mut inner = self.inner.borrow_mut();
        inner.queue.drain(..).collect()
    }

    pub fn is_disconnected(&self) -> bool {
        let inner = self.inner.borrow();
        inner.sender_count == 0 && inner.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.inner.borrow_mut().receiver_alive = false;
    }
}

impl<T> Iterator for Receiver<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self.try_recv() {
            Ok(v) => Some(v),
            Err(_) => None,
        }
    }
}

pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Rc::new(RefCell::new(Inner::new(None)));
    (
        Sender {
            inner: Rc::clone(&inner),
        },
        Receiver { inner },
    )
}

pub fn bounded<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "capacity must be > 0");
    let inner = Rc::new(RefCell::new(Inner::new(Some(capacity))));
    (
        Sender {
            inner: Rc::clone(&inner),
        },
        Receiver { inner },
    )
}

pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Rc::new(RefCell::new(Inner::new(None)));
    (
        Sender {
            inner: Rc::clone(&inner),
        },
        Receiver { inner },
    )
}

// ──────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_send_recv() {
        let (tx, rx) = channel();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        assert_eq!(rx.try_recv(), Ok(1));
        assert_eq!(rx.try_recv(), Ok(2));
        assert_eq!(rx.try_recv(), Ok(3));
        assert_eq!(rx.try_recv(), Err(RecvError::Empty));
    }

    #[test]
    fn disconnected_on_sender_drop() {
        let (tx, rx) = channel::<i32>();
        drop(tx);
        assert_eq!(rx.try_recv(), Err(RecvError::Disconnected));
    }

    #[test]
    fn disconnected_on_receiver_drop() {
        let (tx, rx) = channel();
        drop(rx);
        assert_eq!(tx.send(42), Err(SendError::Disconnected(42)));
    }

    #[test]
    fn multiple_senders() {
        let (tx1, rx) = channel();
        let tx2 = tx1.clone();
        tx1.send("hello").unwrap();
        tx2.send("world").unwrap();
        assert_eq!(rx.drain(), vec!["hello", "world"]);
    }

    #[test]
    fn bounded_full() {
        let (tx, rx) = bounded(2);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(tx.send(3), Err(SendError::Full(3)));
        assert_eq!(rx.try_recv(), Ok(1));
        tx.send(3).unwrap(); // now there's room
    }

    #[test]
    fn iterator() {
        let (tx, rx) = channel();
        for i in 0..5 {
            tx.send(i).unwrap();
        }
        drop(tx);
        let collected: Vec<_> = rx.collect();
        assert_eq!(collected, vec![0, 1, 2, 3, 4]);
    }
}
