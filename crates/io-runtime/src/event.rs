use crossbeam::channel::{Receiver, Sender, unbounded};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::ops::Sub;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;
use std::sync::Mutex;

type BoxedSender = Box<dyn Fn(&dyn Any) + Send + Sync>;

pub struct Subscription<E> {
    receiver: Receiver<E>,
}

impl<E> Subscription<E> {
    pub fn try_recv(&self) -> Option<E> {
        self.receiver.try_recv().ok()
    }

    pub fn recv(&self) -> Option<E> {
        self.receiver.recv().ok()
    }
}

#[derive(Clone)]
pub struct EventManager {
    inner: Arc<EventManagerInner>,
}

impl EventManager {
    pub fn new() -> std::io::Result<Self> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            inner: Arc::new(EventManagerInner {
                senders: Mutex::new(HashMap::new()),
                fd: unsafe { OwnedFd::from_raw_fd(fd) },
            }),
        })
    }

    #[inline]
    pub fn get_eventfd_as_raw_fd(&self) -> RawFd {
        use std::os::unix::io::AsRawFd;
        self.inner.fd.as_raw_fd()
    }

    pub fn signal(&self) -> std::io::Result<()> {
        let val: u64 = 1;
        let buf = val.to_ne_bytes();

        let ret = unsafe {
            libc::write(
                self.inner.fd.as_raw_fd(),
                buf.as_ptr() as *const libc::c_void,
                std::mem::size_of::<u64>(),
            )
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EAGAIN) {
                // Counter is at its maximum; the fd is still readable — treat as OK.
                return Ok(());
            } else {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Register a crossbeam Sender for a specific event type.
    /// Returns the paired Receiver for the consumer to hold.
    pub fn subscribe<E: Clone + Send + 'static>(&mut self) -> Subscription<E> {
        let (tx, rx) = unbounded::<E>();
        let type_id = TypeId::of::<E>();

        let wrapped: BoxedSender = Box::new(move |event: &dyn Any| {
            if let Some(e) = event.downcast_ref::<E>() {
                // Ignore send errors (receiver dropped)
                let _ = tx.send(e.clone());
            }
        });

        self.inner
            .senders
            .lock()
            .unwrap()
            .entry(type_id)
            .or_default()
            .push(wrapped);

        Subscription { receiver: rx }
    }

    /// Publish an event — clones it into every registered channel for that type.
    pub fn publish<E: Clone + Send + 'static>(&self, event: E) {
        let type_id = TypeId::of::<E>();
        if let Some(senders) = self.inner.senders.lock().unwrap().get(&type_id) {
            for sender in senders {
                sender(&event);
            }
        }
        let _ = self.signal(); // wake the dispatcher to process the event
    }
}

pub struct EventManagerInner {
    senders: Mutex<HashMap<TypeId, Vec<BoxedSender>>>,
    fd: OwnedFd,
}
