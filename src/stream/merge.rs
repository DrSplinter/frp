use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use pin_project::pin_project;
use tokio_stream::StreamMap;

pub fn merge<T: MergeStreamTuple>(tuple: T) -> Merge<T::Item> {
    tuple.merge()
}

pub trait MergeStreamTuple {
    type Item;

    fn streams(self) -> Vec<BoxStream<'static, Self::Item>>;

    fn merge(self) -> Merge<Self::Item>
    where
        Self: Sized,
    {
        Merge {
            inner: self.streams().into_iter().enumerate().collect(),
        }
    }
}

impl<S: Stream + Send + 'static> MergeStreamTuple for Vec<S> {
    type Item = S::Item;

    fn streams(self) -> Vec<BoxStream<'static, Self::Item>> {
        self.into_iter().map(|x| x.boxed()).collect()
    }
}

#[pin_project]
#[must_use = "Streams do nothing unless polled"]
pub struct Merge<T> {
    #[pin]
    inner: StreamMap<usize, BoxStream<'static, T>>,
}

impl<T> Stream for Merge<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project()
            .inner
            .poll_next(cx)
            .map(|x| x.map(|(_, x)| x))
    }
}

macro_rules! impl_stream_tuple {
    ($($n:ident),*) => {
        impl<T, $($n),*> MergeStreamTuple for ($($n,)*)
        where
            $($n: Stream<Item = T> + 'static),*,
            $($n: Send + 'static),*,
            T: Send + 'static
        {
            type Item = T;

            fn streams(self) -> Vec<BoxStream<'static, Self::Item>> {
                #[allow(non_snake_case)]
                let ($($n,)*) = self;
                vec![$(Box::pin($n),)*]
            }
        }
    };
    () => {

    };
}

macro_rules! impl_stream_tuples {
    ($first:ident) => {};
    ($first:ident, $($rest:ident),*) => {
        impl_stream_tuple!($first, $($rest),*);
        impl_stream_tuples!($($rest),*);
    };
}

impl_stream_tuples!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use futures::StreamExt as _;

    use super::*;

    #[tokio::test]
    async fn merge_test() {
        let stream1 = futures::stream::iter(vec![1, 2, 3]).boxed();
        let stream2 = futures::stream::iter(vec![4, 5, 6]).boxed();
        let stream3 = futures::stream::iter(vec![7, 8, 9]).boxed();

        let merged = (stream1, stream2, stream3).merge();
        let values: HashSet<_> = merged.collect().await;

        assert_eq!(values, HashSet::from([1, 4, 7, 2, 5, 8, 3, 6, 9]));
    }

    #[tokio::test]
    async fn merge_is_send() {
        let stream1 = futures::stream::iter(vec![1, 2, 3]).boxed();
        let stream2 = futures::stream::iter(vec![4, 5, 6]).boxed();
        let merged = (stream1, stream2).merge();

        tokio::task::spawn(async move {
            let _ = merged.collect::<Vec<_>>().await;
        });
    }
}
