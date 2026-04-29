use std::fmt;

/// Unified error type for the event loop system.
#[derive(Debug)]
pub enum Error {
    DispatcherDisconnected,
    /// Wraps std::io::Error from syscalls or io_uring operations
    Io(std::io::Error),
    /// Submission queue is full; caller should back off and retry
    SubmissionQueueFull,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::SubmissionQueueFull => write!(f, "io_uring submission queue is full"),
            Error::DispatcherDisconnected => write!(f, "dispatcher channel is disconnected"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
