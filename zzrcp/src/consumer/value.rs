/// Return value for value requests.

use std::{sync::{Arc, Mutex}, pin::Pin, task::{Context, Poll}};

use futures::{channel::{mpsc::UnboundedSender, oneshot}, Future, future::FusedFuture, ready, FutureExt};
use pin_project::{pin_project, pinned_drop};

use super::{Message, ResultSender, Aborter, Payload};

/// Future returned by consumer value requests.
#[derive(Debug)]
#[pin_project(PinnedDrop)]
pub struct ValueRequest<T, Request, Error> {
    #[allow(clippy::type_complexity)]
    sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
    id: usize,
    request: Option<Request>,
    receiver: Option<oneshot::Receiver<super::Result<T, Error>>>,
    aborter: Option<Aborter<T, Request, Error>>,
    abort_receiver: Option<oneshot::Receiver<()>>,
}

impl<T, Request, Error> ValueRequest<T, Request, Error> {
    /// Create new value request.
    /// 
    /// **NOTE**: This is used internally by the generated consumers.<br>
    /// You should never have to create it manually yourself.
    #[allow(clippy::type_complexity)]
    pub fn new(
        sender: Arc<UnboundedSender<(Message<Request>, ResultSender<T, Error>)>>,
        id: usize,
        request: Request,
    ) -> Self {
        ValueRequest {
            sender,
            id,
            request: Some(request),
            receiver: None,
            aborter: None,
            abort_receiver: None,
        }
    }

    /// Request id.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Aborter for this value request.
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

impl<T, Request, Error> Future for ValueRequest<T, Request, Error> {
    type Output = super::Result<T, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut me = self.project();
        if let Some(abort_receiver) = me.abort_receiver {
            if let Poll::Ready(result) = abort_receiver.poll_unpin(cx) {
                if result.is_ok() {
                    me.request.take();
                    me.receiver.take();
                    return Poll::Ready(Err(super::super::Error::Aborted));
                }
            }
        }

        if let Some(request) = me.request.take() {
            let (sender, mut receiver) = oneshot::channel();
            let sender = ResultSender::Value(sender);
            let message = Message {
                id: *me.id,
                payload: Payload::Request(request),
            };
            me.sender
                .unbounded_send((message, sender))
                .map_err(|_| { 
                    me.receiver.take();
                    super::super::Error::Shutdown }
                )?;
            match receiver.poll_unpin(cx) {
                Poll::Ready(result) => {
                    me.receiver.take();
                    Poll::Ready(result.map_err(|_| super::super::Error::Dropped)?)
                },
                Poll::Pending => {
                    *me.receiver = Some(receiver);
                    Poll::Pending
                }
            }
        } else if let Some(receiver) = &mut me.receiver {
            let result = ready!(receiver.poll_unpin(cx));
            me.receiver.take();
            Poll::Ready(result.map_err(|_| super::super::Error::Dropped)?)
        } else {
            Poll::Pending
        }
    }
}

impl<T, Request, Error> FusedFuture for ValueRequest<T, Request, Error> {
    fn is_terminated(&self) -> bool {
        self.request.is_none() && self.receiver.is_none()
    }
}

#[pinned_drop]
impl<T, Request, Error> PinnedDrop for ValueRequest<T, Request, Error> {
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
