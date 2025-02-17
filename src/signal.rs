use futures::{stream, FutureExt, Stream, StreamExt};
use futures_signals::signal::{
    Mutable, MutableSignal, MutableSignalCloned, MutableSignalRef, ReadOnlyMutable, Signal,
    SignalExt,
};

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

pub fn pending<A>(value: A) -> impl Signal<Item = A> {
    stream::pending().to_signal(value)
}

pub fn placeholder<A>(init: A) -> Placeholder<A> {
    Placeholder {
        value: Mutable::new(init),
    }
}

pub struct Placeholder<A> {
    value: Mutable<A>,
}

impl<A: Copy> Placeholder<A> {
    pub fn signal(&self) -> MutableSignal<A> {
        self.value.signal()
    }
}

impl<A: Clone> Placeholder<A> {
    pub fn signal_cloned(&self) -> MutableSignalCloned<A> {
        self.value.signal_cloned()
    }
}

impl<A: Send + Sync + 'static> Placeholder<A> {
    pub fn signal_ref<F: FnMut(&A) -> B, B>(&self, f: F) -> MutableSignalRef<A, F> {
        self.value.signal_ref(f)
    }

    pub fn fill(self, changes: impl Stream<Item = A> + Send + 'static) -> ReadOnlyMutable<A> {
        let state = self.value.clone();

        tokio::spawn({
            let state = state.clone();
            changes.for_each(move |value| {
                state.set(value);
                futures::future::ready(())
            })
        });

        state.read_only()
    }
}
