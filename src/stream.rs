use std::future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::{Stream, StreamExt};
use futures_signals::signal::Signal;
use pin_project::pin_project;

mod merge;

pub use merge::*;

pub trait FrpStreamExt: Stream + Sized {
    fn to_signal(self, first: Self::Item) -> impl Signal<Item = Self::Item> {
        ToSignal {
            stream: Some(self),
            first: Some(first),
        }
    }

    fn accumulate<A, F>(self, init: A, f: F) -> impl Signal<Item = A>
    where
        A: Clone,
        F: Fn(A, Self::Item) -> A + 'static,
    {
        self.scan(init.clone(), move |state, next| {
            *state = f(state.clone(), next);
            future::ready(Some(state.clone()))
        })
        .to_signal(init)
    }

    fn merge<S: Stream>(self, other: S) -> impl Stream<Item = Self::Item>
    where
        S: Stream<Item = Self::Item>,
    {
        tokio_stream::StreamExt::merge(self, other)
    }
}

impl<S: Stream> FrpStreamExt for S {}

#[pin_project]
#[derive(Debug)]
#[must_use = "Signals do nothing unless polled"]
pub struct ToSignal<A: Stream> {
    #[pin]
    stream: Option<A>,
    first: Option<A::Item>,
}

impl<A> Signal for ToSignal<A>
where
    A: Stream,
{
    type Item = A::Item;

    fn poll_change(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let mut value = None;

        let done = loop {
            match this
                .stream
                .as_mut()
                .as_pin_mut()
                .map(|stream| stream.poll_next(cx))
            {
                None => {
                    break true;
                }

                Some(Poll::Ready(None)) => {
                    this.stream.set(None);
                    break true;
                }

                Some(Poll::Ready(Some(new_value))) => {
                    value = Some(new_value);
                    continue;
                }

                Some(Poll::Pending) => {
                    break false;
                }
            }
        };

        match value {
            Some(value) => {
                *this.first = None;
                Poll::Ready(Some(value))
            }
            None => {
                if this.first.is_some() {
                    Poll::Ready(this.first.take())
                } else if done {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

pub struct Broadcaster<A> {
    senders: Arc<Mutex<Vec<Box<dyn Fn(&A) -> bool + Send>>>>,
}

impl<A> Clone for Broadcaster<A> {
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
        }
    }
}

impl<A> Broadcaster<A> {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn read_only(&self) -> ReadOnlyBroadcaster<A> {
        ReadOnlyBroadcaster {
            broadcaster: self.clone(),
        }
    }

    fn receiver<F: Fn(&A) -> B + Send + 'static, B: Send + 'static>(
        &self,
        f: F,
    ) -> futures::channel::mpsc::UnboundedReceiver<B> {
        let (sender, receiver) = futures::channel::mpsc::unbounded();

        self.senders
            .lock()
            .unwrap()
            .push(Box::new(move |a| sender.unbounded_send(f(a)).is_ok()));

        receiver
    }

    pub fn send(&self, value: A) {
        let mut senders = self.senders.lock().unwrap();

        senders.retain(|sender| sender(&value))
    }
}

impl<A> Broadcaster<A> {
    pub fn stream_ref<F: Fn(&A) -> B + Send + 'static, B: Send + 'static>(
        &self,
        f: F,
    ) -> BroadcasterStreamRef<B> {
        let receiver = self.receiver(f);

        BroadcasterStreamRef { receiver }
    }
}

impl<A: Copy + Send + 'static> Broadcaster<A> {
    pub fn stream(&self) -> BroadcasterStream<A> {
        let receiver = self.receiver(|a| *a);

        BroadcasterStream { receiver }
    }
}

impl<A: Clone + Send + 'static> Broadcaster<A> {
    pub fn stream_cloned(&self) -> BroadcasterStreamCloned<A> {
        let receiver = self.receiver(Clone::clone);

        BroadcasterStreamCloned { receiver }
    }
}

#[pin_project]
#[derive(Debug)]
#[must_use = "Streams do nothing unless polled"]
pub struct BroadcasterStream<A> {
    #[pin]
    receiver: futures::channel::mpsc::UnboundedReceiver<A>,
}

impl<A> Stream for BroadcasterStream<A> {
    type Item = A;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().receiver.poll_next(cx)
    }
}

#[pin_project]
#[derive(Debug)]
#[must_use = "Streams do nothing unless polled"]
pub struct BroadcasterStreamCloned<A> {
    #[pin]
    receiver: futures::channel::mpsc::UnboundedReceiver<A>,
}

impl<A> Stream for BroadcasterStreamCloned<A> {
    type Item = A;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().receiver.poll_next(cx)
    }
}

#[pin_project]
#[derive(Debug)]
#[must_use = "Streams do nothing unless polled"]
pub struct BroadcasterStreamRef<A> {
    #[pin]
    receiver: futures::channel::mpsc::UnboundedReceiver<A>,
}

impl<A> Stream for BroadcasterStreamRef<A> {
    type Item = A;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.project().receiver.poll_next(cx)
    }
}

pub struct ReadOnlyBroadcaster<A> {
    broadcaster: Broadcaster<A>,
}

impl<A> ReadOnlyBroadcaster<A> {
    pub fn stream_ref<F: Fn(&A) -> B + Send + 'static, B: Send + 'static>(
        &self,
        f: F,
    ) -> BroadcasterStreamRef<B> {
        self.broadcaster.stream_ref(f)
    }
}

impl<A: Copy + Send + 'static> ReadOnlyBroadcaster<A> {
    pub fn stream(&self) -> BroadcasterStream<A> {
        self.broadcaster.stream()
    }
}

impl<A: Clone + Send + 'static> ReadOnlyBroadcaster<A> {
    pub fn stream_cloned(&self) -> BroadcasterStreamCloned<A> {
        self.broadcaster.stream_cloned()
    }
}

pub fn placeholder<A>() -> Placeholder<A> {
    Placeholder {
        broadcaster: Broadcaster::new(),
    }
}

pub struct Placeholder<A> {
    broadcaster: Broadcaster<A>,
}

impl<A: Copy + Send + 'static> Placeholder<A> {
    pub fn stream(&self) -> BroadcasterStream<A> {
        self.broadcaster.stream()
    }
}

impl<A: 'static> Placeholder<A> {
    pub fn fill(self, stream: impl Stream<Item = A> + Send + 'static) -> ReadOnlyBroadcaster<A> {
        let broadcaster = self.broadcaster.clone();

        tokio::spawn({
            stream.for_each(move |value| {
                self.broadcaster.send(value);

                futures::future::ready(())
            })
        });

        broadcaster.read_only()
    }
}

#[cfg(test)]
mod tests {
    use futures::{stream, StreamExt as _};
    use futures_signals::signal::SignalExt;

    use super::*;

    #[tokio::test]
    async fn test_hold_with_empty_stream() {
        let signal = stream::iter(vec![]).to_signal(0);

        let mut changes = signal.to_stream();

        assert_eq!(changes.next().await, Some(0));
        assert_eq!(changes.next().await, None);
    }

    #[tokio::test]
    async fn test_hold_with_single_value() {
        let signal = stream::iter(vec![1]).to_signal(0);

        let mut changes = signal.to_stream();

        assert_eq!(changes.next().await, Some(1));
        assert_eq!(changes.next().await, None);
    }
}
