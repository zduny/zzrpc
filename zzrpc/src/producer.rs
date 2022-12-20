/// Producer related functionality.
use std::{marker::PhantomData, time::Duration};

use futures::{
    future::{pending, Pending},
    Future,
};
use serde::{Deserialize, Serialize};

use crate::consumer;

use super::{DefaultReceiveErrorCallback, DefaultSendErrorCallback, ShutdownType};

/// Configuration for [produce] method.
///
/// [produce]: self::Produce::produce
#[derive(Debug)]
pub struct Configuration<Shutdown, Error, SendErrorCallback, ReceiveErrorCallback>
where
    Shutdown: Future<Output = ShutdownType>,
    SendErrorCallback: crate::SendErrorCallback<Error>,
    ReceiveErrorCallback: crate::ReceiveErrorCallback<Error>,
{
    /// When this future resolves producers stops producing and returns.
    ///
    /// **NOTE**: By default it is set with [futures::future::Pending], but it doesn't
    /// mean producer will never stop - it will still stop when transport is closed,
    /// or it can be stopped by provided `send_error_callback` or `receive_error_callback`.
    ///
    /// This future is intended for triggering shutdown manually.
    pub shutdown: Shutdown,

    /// This callback is called whenever producer encounters en error while trying to send
    /// message using transport.
    pub send_error_callback: SendErrorCallback,

    /// This callback is called whenever producer encounters en error while trying to receive
    /// a message from transport.
    pub receive_error_callback: ReceiveErrorCallback,

    /// Optional duration after which producer will shutdown if it will not receive
    /// any messages during that period (and there are no requests pending).
    ///
    /// **NOTE**: It resets on every message received - so a producer that has received messages
    /// in the past can still time out when a period of `duration` length with no messages occurs.
    pub timeout: Option<Duration>,

    /// Transport error type marker.
    pub _error: PhantomData<Error>,
}

impl<Error> Default
    for Configuration<
        Pending<ShutdownType>,
        Error,
        DefaultSendErrorCallback,
        DefaultReceiveErrorCallback,
    >
{
    fn default() -> Self {
        Self {
            shutdown: pending(),
            send_error_callback: DefaultSendErrorCallback {},
            receive_error_callback: DefaultReceiveErrorCallback {},
            timeout: Default::default(),
            _error: Default::default(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;

#[cfg(target_arch = "wasm32")]
use js_utils::spawn::JoinHandle;

/// Producer message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message<Response> {
    /// Response to request.
    Response {
        /// Request id.
        id: usize,

        /// Response value.
        response: Response,
    },

    /// Producer was shutdown with [ShutdownType::Aborted].
    Aborted,

    /// Producer was shutdown with [ShutdownType::Shutdown].
    Shutdown,
}

/// Trait implemented by producers.
///
/// You should never have to implement it manually - use derive macro.
pub trait Produce: Sized {
    type Request;
    type Response;

    /// Produce using given transport and configuration.
    ///
    /// **NOTE**: It simply calls [Produce::produce_unreliable], but has different constraints.<br>
    /// Separation is done to ensure user is aware that using unreliable transport will result
    /// in unreliable producer - responses may never reach their consumer.
    #[cfg(not(target_arch = "wasm32"))]
    fn produce<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
        self,
        transport: Transport,
        configuration: Configuration<Shutdown, Error, SendErrorCallback, ReceiveErrorCallback>,
    ) -> JoinHandle<ShutdownType>
    where
        Transport: mezzenger::Transport<consumer::Message<Self::Request>, Message<Self::Response>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + Send
            + 'static,
        Shutdown: Future<Output = ShutdownType> + Send + 'static,
        SendErrorCallback: crate::SendErrorCallback<Error> + Send + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + Send + 'static,
    {
        Self::produce_unreliable(self, transport, configuration)
    }

    /// Produce using given transport and configuration.
    ///
    /// **NOTE**: It simply calls [Produce::produce_unreliable], but has different constraints.<br>
    /// Separation is done to ensure user is aware that using unreliable transport will result
    /// in unreliable producer - responses may never reach their consumer.
    #[cfg(target_arch = "wasm32")]
    fn produce<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
        self,
        transport: Transport,
        configuration: Configuration<Shutdown, Error, SendErrorCallback, ReceiveErrorCallback>,
    ) -> JoinHandle<ShutdownType>
    where
        Transport: mezzenger::Transport<consumer::Message<Self::Request>, Message<Self::Response>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + 'static,
        Shutdown: Future<Output = ShutdownType> + 'static,
        SendErrorCallback: crate::SendErrorCallback<Error> + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + 'static,
    {
        Self::produce_unreliable(self, transport, configuration)
    }

    /// Produce using given transport and configuration.
    ///
    /// **NOTE**: If unreliable transport is used, producer will inherit its unreliability -
    /// producer messages may never reach their destination.
    #[cfg(not(target_arch = "wasm32"))]
    fn produce_unreliable<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
        self,
        transport: Transport,
        configuration: Configuration<Shutdown, Error, SendErrorCallback, ReceiveErrorCallback>,
    ) -> JoinHandle<ShutdownType>
    where
        Transport: mezzenger::Transport<consumer::Message<Self::Request>, Message<Self::Response>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + Send
            + 'static,
        Shutdown: Future<Output = ShutdownType> + Send + 'static,
        SendErrorCallback: crate::SendErrorCallback<Error> + Send + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + Send + 'static;

    /// Produce using given transport and configuration.
    ///
    /// **NOTE**: If unreliable transport is used, producer will inherit its unreliability -
    /// producer messages may never reach their destination.
    #[cfg(target_arch = "wasm32")]
    fn produce_unreliable<Transport, Error, Shutdown, SendErrorCallback, ReceiveErrorCallback>(
        self,
        transport: Transport,
        configuration: Configuration<Shutdown, Error, SendErrorCallback, ReceiveErrorCallback>,
    ) -> JoinHandle<ShutdownType>
    where
        Transport: mezzenger::Transport<consumer::Message<Self::Request>, Message<Self::Response>, Error>
            + mezzenger::Reliable
            + mezzenger::Order
            + 'static,
        Shutdown: Future<Output = ShutdownType> + 'static,
        SendErrorCallback: crate::SendErrorCallback<Error> + 'static,
        ReceiveErrorCallback: crate::ReceiveErrorCallback<Error> + 'static;
}
