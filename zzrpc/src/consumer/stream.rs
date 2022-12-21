//! Return value for streaming requests.

use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures::{
    channel::{
        mpsc::{unbounded, UnboundedReceiver, UnboundedSender},
        oneshot,
    },
    future::FusedFuture,
    ready,
    stream::FusedStream,
    Future, FutureExt, StreamExt,
};
use pin_project::{pin_project, pinned_drop};

use super::{Aborter, Message, Payload, ResultSender};

/// Future returned by consumer streaming requests.
#[derive(Debug)]
#[pin_project(PinnedDrop)]
pub struct StreamRequest<T, Request, Error> {
    #[allow(clippy::type_complexity)]
    sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
    id: usize,
    request: Option<Request>,
    result_receiver: Option<oneshot::Receiver<super::Result<(), Error>>>,
    values_receiver: Option<UnboundedReceiver<T>>,
    aborter: Option<Aborter<T, Request, Error>>,
    abort_receiver: Option<oneshot::Receiver<()>>,
}

impl<T, Request, Error> StreamRequest<T, Request, Error> {
    /// Create new stream request.
    ///
    /// **NOTE**: This is used internally by the generated consumers.<br>
    /// You should never have to create it manually yourself.
    #[allow(clippy::type_complexity)]
    pub fn new(
        sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
        id: usize,
        request: Request,
    ) -> Self {
        StreamRequest {
            sender,
            id,
            request: Some(request),
            result_receiver: None,
            values_receiver: None,
            aborter: None,
            abort_receiver: None,
        }
    }

    /// Request id.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Aborter for this stream request.
    ///
    /// **NOTE**: This aborter has ability to abort resulting stream as well.
    pub fn aborter(&mut self) -> Aborter<T, Request, Error> {
        let aborter = self.aborter.get_or_insert_with(|| {
            let (abort_sender, abort_receiver) = oneshot::channel();
            self.abort_receiver = Some(abort_receiver);
            Aborter {
                id: self.id,
                sender: self.sender.clone(),
                abort_sender: Arc::new(Mutex::new(Some(abort_sender))),
            }
        });
        aborter.clone()
    }
}

impl<T, Request, Error> Future for StreamRequest<T, Request, Error> {
    type Output = super::Result<Stream<T, Request, Error>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut me = self.project();
        if let Some(abort_receiver) = &mut me.abort_receiver {
            if let Poll::Ready(result) = abort_receiver.poll_unpin(cx) {
                if result.is_ok() {
                    me.request.take();
                    me.result_receiver.take();
                    me.values_receiver.take();
                    return Poll::Ready(Err(super::super::Error::Aborted));
                }
            }
        }

        if let Some(request) = me.request.take() {
            let (result_sender, mut result_receiver) = oneshot::channel();
            let (values_sender, values_receiver) = unbounded();

            let sender = ResultSender::Stream {
                result_sender,
                values_sender,
            };
            let message = Message {
                id: *me.id,
                payload: Payload::Request(request),
            };
            me.sender.unbounded_send((message, sender)).map_err(|_| {
                me.result_receiver.take();
                super::super::Error::Shutdown
            })?;
            match result_receiver.poll_unpin(cx) {
                Poll::Ready(result) => {
                    me.result_receiver.take();
                    let result = result
                        .map(|_| Stream {
                            id: *me.id,
                            sender: me.sender.clone(),
                            receiver: Some(values_receiver),
                            aborter: me.aborter.take(),
                            abort_receiver: me.abort_receiver.take(),
                        })
                        .map_err(|_| {
                            me.values_receiver.take();
                            super::super::Error::Dropped
                        });
                    Poll::Ready(result)
                }
                Poll::Pending => {
                    *me.result_receiver = Some(result_receiver);
                    *me.values_receiver = Some(values_receiver);
                    Poll::Pending
                }
            }
        } else if let Some(receiver) = &mut me.result_receiver {
            let result = ready!(receiver.poll_unpin(cx));
            me.result_receiver.take();
            let result = result
                .map(|_| Stream {
                    id: *me.id,
                    sender: me.sender.clone(),
                    receiver: me.values_receiver.take(),
                    aborter: me.aborter.take(),
                    abort_receiver: me.abort_receiver.take(),
                })
                .map_err(|_| {
                    me.values_receiver.take();
                    super::super::Error::Dropped
                });
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    }
}

impl<T, Request, Error> FusedFuture for StreamRequest<T, Request, Error> {
    fn is_terminated(&self) -> bool {
        self.request.is_none() && self.result_receiver.is_none()
    }
}

#[pinned_drop]
impl<T, Request, Error> PinnedDrop for StreamRequest<T, Request, Error> {
    fn drop(self: Pin<&mut Self>) {
        if !self.is_terminated() {
            let payload = Payload::Abort;
            let message = Message {
                id: self.id,
                payload,
            };
            let _ = self.sender.unbounded_send((message, ResultSender::Abort));
        }
    }
}

/// Stream returned by the [StreamRequest].
#[derive(Debug)]
pub struct Stream<T, Request, Error> {
    id: usize,
    #[allow(clippy::type_complexity)]
    sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
    receiver: Option<UnboundedReceiver<T>>,
    aborter: Option<Aborter<T, Request, Error>>,
    abort_receiver: Option<oneshot::Receiver<()>>,
}

impl<T, Request, Error> Stream<T, Request, Error> {
    /// Aborter for this stream.
    pub fn aborter(&mut self) -> Aborter<T, Request, Error> {
        let aborter = self.aborter.get_or_insert_with(|| {
            let (abort_sender, abort_receiver) = oneshot::channel();
            self.abort_receiver = Some(abort_receiver);
            Aborter {
                id: self.id,
                sender: self.sender.clone(),
                abort_sender: Arc::new(Mutex::new(Some(abort_sender))),
            }
        });
        aborter.clone()
    }
}

impl<T, Request, Error> futures::Stream for Stream<T, Request, Error> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(receiver) = &mut self.receiver {
            match receiver.poll_next_unpin(cx) {
                Poll::Ready(result) => {
                    if let Some(result) = result {
                        Poll::Ready(Some(result))
                    } else {
                        self.receiver.take();
                        Poll::Ready(None)
                    }
                }
                Poll::Pending => {
                    if let Some(abort_receiver) = &mut self.abort_receiver {
                        if let Poll::Ready(result) = abort_receiver.poll_unpin(cx) {
                            if result.is_ok() {
                                self.receiver.take();
                                return Poll::Ready(None);
                            }
                        }
                    }
                    Poll::Pending
                }
            }
        } else {
            Poll::Ready(None)
        }
    }
}

impl<T, Request, Error> FusedStream for Stream<T, Request, Error> {
    fn is_terminated(&self) -> bool {
        self.receiver.is_some()
    }
}

impl<Request, Output, Error> Drop for Stream<Request, Output, Error> {
    fn drop(&mut self) {
        if !self.is_terminated() {
            let payload = Payload::Abort;
            let message = Message {
                id: self.id,
                payload,
            };
            let _ = self.sender.unbounded_send((message, ResultSender::Abort));
        }
    }
}
