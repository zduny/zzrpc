/// RPC over [mezzenger](https://github.com/zduny/mezzenger) transports.

use std::{
    fmt::{Debug, Display},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{future::FusedFuture, ready, Future, FutureExt};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use tokio::time::{sleep, Sleep};
#[cfg(not(target_arch = "wasm32"))]
pub use tokio::spawn;
#[cfg(not(target_arch = "wasm32"))]
pub type JoinHandle<T> = tokio::task::JoinHandle<T>;

#[cfg(target_arch = "wasm32")]
use js_utils::{sleep, sleep::Sleep};
#[cfg(target_arch = "wasm32")]
use zduny_wasm_timer::Instant;
#[cfg(target_arch = "wasm32")]
pub use js_utils::spawn::spawn;
#[cfg(target_arch = "wasm32")]
pub type JoinHandle<T> = js_utils::spawn::JoinHandle<T>;

pub use zzrpc_derive::Produce;

pub mod consumer;
pub mod producer;

/// RPC error.
#[derive(Debug, Clone)]
pub enum Error<TransportError> {
    /// Connection closed.
    Closed,

    /// Request was aborted by consumer/producer.
    Aborted,

    /// Consumer/producer was shut down.
    ///
    /// **NOTE**: It may also be caused by connection being closed before consumer
    /// request was made.
    Shutdown,

    /// Consumer timed out.
    Timeout,

    /// Consumer was dropped before request could complete.
    Dropped,

    /// Request message transport error.
    Transport(TransportError),
}

impl<TransportError> From<ShutdownType> for Error<TransportError> {
    fn from(value: ShutdownType) -> Self {
        match value {
            ShutdownType::Closed => Error::Closed,
            ShutdownType::Shutdown => Error::Shutdown,
            ShutdownType::Aborted => Error::Aborted,
            ShutdownType::Timeout => Error::Timeout,
        }
    }
}

impl<Transport> Display for Error<Transport>
where
    Transport: Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Closed => write!(f, "transport closed"),
            Error::Aborted => write!(f, "request was aborted"),
            Error::Shutdown => write!(f, "producer/consumer was shutdown"),
            Error::Timeout => write!(f, "producer/consumer timed out"),
            Error::Dropped => write!(f, "consumer was dropped"),
            Error::Transport(error) => write!(f, "transport error: {error}"),
        }
    }
}

impl<Transport> std::error::Error for Error<Transport> where Transport: Debug + Display {}

/// Cause of producer/consumer shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownType {
    /// Connection closed.
    Closed,

    /// Manual shutdown.
    Shutdown,

    /// Abort all requests.
    Aborted,

    /// Consumer/producer timed out.
    ///
    /// **NOTE**: Consumer requests will not receive [Error::Timeout] error
    /// (they will error out with other error type like [Error::Closed] or [Error::Shutdown]).
    /// This design is deliberate to not give consumers easily determinable info about timeout
    /// duration producer was configured with.
    Timeout,
}

/// Send/receive error handling strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlingStrategy {
    /// Stop the producer/consumer simulating provided [ShutdownType].
    Stop(ShutdownType),

    /// Ignore the error (and keep running).
    Ignore,
}

/// Callback called when producer encounters an error while trying to send a message
/// through transport.
///
/// **NOTE**: it also receives [mezzenger::Error::Closed] messages and they need to be properly
/// handled (most likely by returning [HandlingStrategy::Stop]).
pub trait SendErrorCallback<Error> {
    /// Handle sending error.
    ///
    /// **NOTE**: it also receives [mezzenger::Error::Closed] messages and they need to be properly
    /// handled (most likely by returning [HandlingStrategy::Stop]).    
    fn on_send_error(
        &mut self,
        request_id: usize,
        error: mezzenger::Error<Error>,
    ) -> HandlingStrategy;
}

impl<T, Error> SendErrorCallback<Error> for T
where
    T: FnMut(usize, mezzenger::Error<Error>) -> HandlingStrategy,
{
    fn on_send_error(
        &mut self,
        request_id: usize,
        error: mezzenger::Error<Error>,
    ) -> HandlingStrategy {
        (self)(request_id, error)
    }
}

/// Callback called when producer/consumer encounters an error while trying to receive a message
/// from transport.
pub trait ReceiveErrorCallback<Error> {
    /// Handle receiving error.
    fn on_receive_error(&mut self, error: Error) -> HandlingStrategy;
}

impl<T, Error> ReceiveErrorCallback<Error> for T
where
    T: FnMut(Error) -> HandlingStrategy,
{
    fn on_receive_error(&mut self, error: Error) -> HandlingStrategy {
        (self)(error)
    }
}

/// Default [SendErrorCallback].
/// 
/// It shutdowns producer on [mezzenger::Error::Closed] error with [ShutdownType::Closed]. <br>
/// It ignores all other types of errors.
#[derive(Debug)]
pub struct DefaultSendErrorCallback {}

impl<Error> SendErrorCallback<Error> for DefaultSendErrorCallback {
    fn on_send_error(
        &mut self,
        _request_id: usize,
        error: mezzenger::Error<Error>,
    ) -> HandlingStrategy {
        match error {
            mezzenger::Error::Closed => HandlingStrategy::Stop(ShutdownType::Closed),
            mezzenger::Error::Other(_) => HandlingStrategy::Ignore,
        }
    }
}

/// Default [ReceiveErrorCallback].
/// 
/// It ignores errors.
#[derive(Debug)]
pub struct DefaultReceiveErrorCallback {}

impl<Error> ReceiveErrorCallback<Error> for DefaultReceiveErrorCallback {
    fn on_receive_error(&mut self, _error: Error) -> HandlingStrategy {
        HandlingStrategy::Ignore
    }
}

/// Timeout future.
/// 
/// It is used internally by producers/consumers.<br>
/// You should never have to construct it yourself.
#[derive(Debug)]
pub enum Timeout {
    /// Timeout with duration.
    Duration {
        /// Timeout duration.
        duration: Duration,

        /// Sleep future.
        sleep: Pin<Box<Sleep>>,

        /// Is timeout future terminated.
        terminated: bool,
    },

    /// Timeout that never occurs.
    Never,
}

impl Timeout {
    /// Create new timeout with specified duration.
    pub fn new(duration: Duration) -> Self {
        Timeout::Duration {
            duration,
            sleep: Box::pin(sleep(duration)),
            terminated: false,
        }
    }

    /// Create new timeout that never occurs.
    pub fn never() -> Self {
        Timeout::Never
    }

    /// Reset timeout (with duration it was created with).
    pub fn reset(&mut self) {
        if let Timeout::Duration {
            duration, sleep, ..
        } = self
        {
            let deadline = Instant::now() + *duration;
            sleep.as_mut().reset(deadline.into())
        }
    }
}

impl Future for Timeout {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut *self {
            Timeout::Duration {
                sleep, terminated, ..
            } => {
                ready!(sleep.poll_unpin(cx));
                *terminated = true;
                Poll::Ready(())
            }
            Timeout::Never => Poll::Pending,
        }
    }
}

impl FusedFuture for Timeout {
    fn is_terminated(&self) -> bool {
        match self {
            Timeout::Duration { terminated, .. } => *terminated,
            Timeout::Never => false,
        }
    }
}
