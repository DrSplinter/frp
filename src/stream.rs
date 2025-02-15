use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
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
