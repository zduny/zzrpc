/// Consumer related functionality.
mod value;
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{
    channel::{mpsc::UnboundedSender, oneshot},
    future::{pending, Pending},
    Future,
};
use serde::{Deserialize, Serialize};
pub use value::*;

mod stream;
pub use stream::*;

use super::{DefaultReceiveErrorCallback, ShutdownType};

use crate::producer;

/// Consumer message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message<Request> {
    /// Request id.
    pub id: usize,

    /// Message payload.
    pub payload: Payload<Request>,
}

/// Consumer message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload<Request> {
    /// Request arguments.
    Request(Request),

    /// Abort request.
    Abort,
}

/// Result type for consumer methods.
pub type Result<T, Error> = std::result::Result<T, super::Error<Error>>;

/// Configuration for [consume] method.
///
/// [consume]: self::Consume::consume
#[derive(Debug)]
pub struct Configuration<Shutdown, Error, ReceiveErrorCallback>
where
    Shutdown: Future<Output = ShutdownType>,
    ReceiveErrorCallback: crate::ReceiveErrorCallback<Error>,
{
    pub shutdown: Shutdown,
    pub receive_error_callback: ReceiveErrorCallback,
    pub timeout: Option<Duration>,
    pub _error: PhantomData<Error>,
}

impl<Error> Default for Configuration<Pending<ShutdownType>, Error, DefaultReceiveErrorCallback> {
    fn default() -> Self {
        Self {
            shutdown: pending(),
            receive_error_callback: DefaultReceiveErrorCallback {},
            timeout: Default::default(),
            _error: Default::default(),
        }
    }
}

/// Used internally by derived consumer implementations to pass values around.
///
/// You should never have to create it manually yourself.
#[derive(Debug)]
pub enum ResultSender<T, Error> {
    Value(oneshot::Sender<Result<T, Error>>),
    Stream {
        result_sender: oneshot::Sender<Result<(), Error>>,
        values_sender: UnboundedSender<T>,
    },
    Abort,
}

/// Responsible for aborting requests/streams.
#[derive(Debug)]
pub struct Aborter<T, Request, Error> {
    id: usize,
    #[allow(clippy::type_complexity)]
    sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
    abort_sender: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl<Request, Output, Error> Aborter<Request, Output, Error> {
    /// Abort value request/stream request/stream.
    ///
    /// **NOTE** regarding streams: Values that were already received from the producer
    /// before abort has been completed will still be returned by the stream.
    pub fn abort(self) {
        let mut abort_sender = self.abort_sender.lock().unwrap();
        if let Some(abort_sender) = abort_sender.take() {
            let payload = Payload::Abort;
            let message = Message {
                id: self.id,
                payload,
            };
            let _ = self.sender.unbounded_send((message, ResultSender::Abort));
            let _ = abort_sender.send(());
        }
    }

    /// Id of the request this [Aborter] can abort.
    pub fn request_id(&self) -> usize {
        self.id
    }
}

impl<Request, Output, Error> Clone for Aborter<Request, Output, Error> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            sender: self.sender.clone(),
            abort_sender: self.abort_sender.clone(),
        }
    }
}

/// Trait implemented by consumers.
///
/// You should never have to implement it manually - use macros.
pub trait Consume<Consumer, Error> {
    type Request;
    type Response;

    /// Create consumer using given transport and configuration.
    ///
    /// **NOTE**: It simply calls [Consume::consume_unreliable], but has different constraints.<br>
    /// Separation is done to ensure user is aware that using unreliable transport will result
    /// in unreliable consumer - consumer methods may never resolve as producer/consumer messages
    /// may be lost in transport.
    #[cfg(not(target_arch = "wasm32"))]
    fn consume<Transport, Shutdown, ReceiveErrorCallback>(
        transport: Transport,
        configuration: Configuration<Shutdown, Error, ReceiveErrorCallback>,
    ) -> Consumer
    where
        Transport: mezzenger::Transport<producer::Message<Self::Response>, Message<Self::Request>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + Send
            + 'static,
        Shutdown: Future<Output = ShutdownType> + Send + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + Send + 'static,
        Error: Send + 'static,
    {
        Self::consume_unreliable(transport, configuration)
    }

    /// Create consumer using given transport and configuration.
    ///
    /// **NOTE**: If unreliable transport is used, consumer will inherit its unreliability -
    /// consumer methods may never resolve due to messages being lost in transport.
    #[cfg(not(target_arch = "wasm32"))]
    fn consume_unreliable<Transport, Shutdown, ReceiveErrorCallback>(
        transport: Transport,
        configuration: Configuration<Shutdown, Error, ReceiveErrorCallback>,
    ) -> Consumer
    where
        Transport: mezzenger::Transport<producer::Message<Self::Response>, Message<Self::Request>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + Send
            + 'static,
        Shutdown: Future<Output = ShutdownType> + Send + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + Send + 'static,
        Error: Send + 'static;

    /// Create consumer using given transport and configuration.
    ///
    /// **NOTE**: It simply calls [Consume::consume_unreliable], but has different constraints.<br>
    /// Separation is done to ensure user is aware that using unreliable transport will result
    /// in unreliable consumer - consumer methods may never resolve as producer/consumer messages
    /// may be lost in transport.
    #[cfg(target_arch = "wasm32")]
    fn consume<Transport, Shutdown, ReceiveErrorCallback>(
        transport: Transport,
        configuration: Configuration<Shutdown, Error, ReceiveErrorCallback>,
    ) -> Self::Consumer
    where
        Transport: mezzenger::Transport<producer::Message<Self::Response>, Message<Self::Request>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + 'static,
        Shutdown: Future<Output = ShutdownType> + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + 'static,
        Error: 'static,
    {
        Self::consume_unreliable(transport, configuration)
    }

    /// Create consumer using given transport and configuration.
    ///
    /// **NOTE**: If unreliable transport is used, consumer will inherit its unreliability -
    /// consumer methods may never resolve due to messages being lost in transport.
    #[cfg(target_arch = "wasm32")]
    fn consume_unreliable<Transport, Shutdown, ReceiveErrorCallback>(
        transport: Transport,
        configuration: Configuration<Shutdown, Error, ReceiveErrorCallback>,
    ) -> Self::Consumer
    where
        Transport: mezzenger::Transport<producer::Message<Self::Response>, Message<Self::Request>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + 'static,
        Shutdown: Future<Output = ShutdownType> + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + 'static,
        Error: 'static;
}
