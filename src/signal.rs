use futures::{stream, FutureExt, Stream, StreamExt};
use futures_signals::signal::{Mutable, ReadOnlyMutable, Signal, SignalExt};

use crate::stream::FrpStreamExt;

pub trait FrpSignalExt: Signal {
    fn materialize(self) -> ReadOnlyMutable<Self::Item>
    where
        Self: Sized + Send + 'static,
        Self::Item: Send + Sync + 'static,
    {
        let (first, rest) = self
            .to_stream()
            .boxed()
            .into_future()
            .now_or_never()
            .unwrap();

        let state = Mutable::new(first.unwrap());

        tokio::spawn({
            let state = state.clone();
            rest.for_each(move |value| {
                state.set(value);
                futures::future::ready(())
            })
        });

        state.read_only()
    }
}

impl<S: Signal> FrpSignalExt for S {}

pub fn pending<T>(value: T) -> impl Signal<Item = T> {
    stream::pending().to_signal(value)
}

pub fn recursive<T, F, S>(init: T, get_changes: F) -> ReadOnlyMutable<T>
where
    T: Copy + Send + Sync + 'static,
    F: FnOnce(ReadOnlyMutable<T>) -> S,
    S: Stream<Item = T> + Send + 'static,
{
    let state = Mutable::new(init);
    let changes = get_changes(state.read_only());

    tokio::spawn({
        let state = state.clone();
        changes.for_each(move |value| {
            state.set(value);
            futures::future::ready(())
        })
    });

    state.read_only()
}
