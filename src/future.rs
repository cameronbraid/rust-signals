use std::pin::{Pin, Unpin};
// TODO use parking_lot ?
use std::sync::{Arc, Weak, Mutex};
// TODO use parking_lot ?
use std::sync::atomic::{AtomicBool, Ordering};
use futures_core::task::{LocalWaker, Waker};
use futures_core::Poll;
use futures_core::future::Future;
use pin_utils::{unsafe_pinned, unsafe_unpinned};
use discard::{Discard, DiscardOnDrop};


#[derive(Debug)]
struct CancelableFutureState {
    is_cancelled: AtomicBool,
    waker: Mutex<Option<Waker>>,
}


#[derive(Debug)]
pub struct CancelableFutureHandle {
    state: Weak<CancelableFutureState>,
}

impl Discard for CancelableFutureHandle {
    fn discard(self) {
        if let Some(state) = self.state.upgrade() {
            let mut lock = state.waker.lock().unwrap();

            // TODO verify that this is correct
            state.is_cancelled.store(true, Ordering::SeqCst);

            if let Some(waker) = lock.take() {
                drop(lock);
                waker.wake();
            }
        }
    }
}


#[derive(Debug)]
#[must_use = "Futures do nothing unless polled"]
pub struct CancelableFuture<A, B> {
    state: Arc<CancelableFutureState>,
    future: A,
    when_cancelled: Option<B>,
}

impl<A, B> CancelableFuture<A, B> {
    unsafe_pinned!(future: A);
    unsafe_unpinned!(when_cancelled: Option<B>);
}

impl<A, B> Unpin for CancelableFuture<A, B> where A: Unpin {}

impl<A, B> Future for CancelableFuture<A, B>
    where A: Future,
          B: FnOnce() -> A::Output {

    type Output = A::Output;

    // TODO should this inline ?
    #[inline]
    fn poll(mut self: Pin<&mut Self>, waker: &LocalWaker) -> Poll<Self::Output> {
        // TODO is this correct ?
        if self.state.is_cancelled.load(Ordering::SeqCst) {
            let callback = self.when_cancelled().take().unwrap();
            // TODO figure out how to call the callback immediately when discard is called, e.g. using two Arc<Mutex<>>
            Poll::Ready(callback())

        } else {
            match self.future().poll(waker) {
                Poll::Pending => {
                    // TODO is this correct ?
                    *self.state.waker.lock().unwrap() = Some(waker.clone().into_waker());
                    Poll::Pending
                },
                a => a,
            }
        }
    }
}


// TODO figure out a more efficient way to implement this
// TODO replace with futures_util::abortable ?
pub fn cancelable_future<A, B>(future: A, when_cancelled: B) -> (DiscardOnDrop<CancelableFutureHandle>, CancelableFuture<A, B>)
    where A: Future,
          B: FnOnce() -> A::Output {

    let state = Arc::new(CancelableFutureState {
        is_cancelled: AtomicBool::new(false),
        waker: Mutex::new(None),
    });

    let cancel_handle = DiscardOnDrop::new(CancelableFutureHandle {
        state: Arc::downgrade(&state),
    });

    let cancel_future = CancelableFuture {
        state,
        future,
        when_cancelled: Some(when_cancelled),
    };

    (cancel_handle, cancel_future)
}
