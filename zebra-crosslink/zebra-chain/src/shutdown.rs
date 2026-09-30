//! Shutdown related code.
//!
//! A global flag indicates when the application is shutting down so actions can be taken
//! at different parts of the codebase.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    task::{Context, Poll, Waker},
};

static SHUTDOWN_WAITERS: Mutex<Vec<Waker>> = Mutex::new(Vec::new());

/// A flag to indicate if Zebra is shutting down.
///
/// Initialized to `false` at startup.
pub static IS_SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// Returns true if the application is shutting down.
///
/// Returns false otherwise.
pub fn is_shutting_down() -> bool {
    // ## Correctness:
    //
    // Since we're shutting down, and this is a one-time operation,
    // performance is not important. So we use the strongest memory
    // ordering.
    // https://doc.rust-lang.org/nomicon/atomics.html#sequentially-consistent
    IS_SHUTTING_DOWN.load(Ordering::SeqCst)
}

/// Requests cooperative shutdown and wakes every registered async waiter.
///
/// This is idempotent, but is not an OS signal handler: waking a future can lock or allocate.
pub fn set_shutting_down() {
    IS_SHUTTING_DOWN.store(true, Ordering::SeqCst);
    wake_waiters(&SHUTDOWN_WAITERS);
}

/// Waits for a shutdown request from a signal, the GUI, or another node component.
///
/// Requests are latched, so a future created after shutdown starts completes immediately.
pub async fn shutdown_requested() {
    std::future::poll_fn(|cx| poll_shutdown(&IS_SHUTTING_DOWN, &SHUTDOWN_WAITERS, cx)).await;
}

fn poll_shutdown(
    flag: &AtomicBool,
    waiters: &Mutex<Vec<Waker>>,
    cx: &mut Context<'_>,
) -> Poll<()> {
    let mut waiters = waiters.lock().unwrap_or_else(|error| error.into_inner());
    if flag.load(Ordering::SeqCst) {
        return Poll::Ready(());
    }
    if !waiters.iter().any(|waker| waker.will_wake(cx.waker())) {
        waiters.push(cx.waker().clone());
    }
    Poll::Pending
}

fn wake_waiters(waiters: &Mutex<Vec<Waker>>) {
    let waiters = std::mem::take(&mut *waiters.lock().unwrap_or_else(|error| error.into_inner()));
    // Never wake under the mutex: an executor may immediately poll the future again.
    for waker in waiters {
        waker.wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Arc};
    use std::task::Wake;

    #[derive(Default)]
    struct Counter(AtomicUsize);

    impl Wake for Counter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn shutdown_is_latched_and_wakes_all_waiters() {
        let flag = AtomicBool::new(false);
        let waiters = Mutex::new(Vec::new());
        let first = Arc::new(Counter::default());
        let second = Arc::new(Counter::default());
        let first_waker = Waker::from(first.clone());
        let second_waker = Waker::from(second.clone());
        let mut first_context = Context::from_waker(&first_waker);
        let mut second_context = Context::from_waker(&second_waker);
        assert!(poll_shutdown(&flag, &waiters, &mut first_context).is_pending());
        assert!(poll_shutdown(&flag, &waiters, &mut first_context).is_pending());
        assert!(poll_shutdown(&flag, &waiters, &mut second_context).is_pending());
        flag.store(true, Ordering::SeqCst);
        wake_waiters(&waiters);
        wake_waiters(&waiters);
        assert_eq!(first.0.load(Ordering::SeqCst), 1);
        assert_eq!(second.0.load(Ordering::SeqCst), 1);
        assert!(poll_shutdown(&flag, &waiters, &mut first_context).is_ready());
        assert!(waiters.lock().unwrap().is_empty());
    }
}
