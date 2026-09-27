//! The async runtime seam: task spawning, timers, and clocks under tokio's
//! names, so the harness runs unchanged on a native tokio runtime and inside a
//! browser.
//!
//! Native builds re-export tokio and `std::time` directly; nothing changes
//! there. WebAssembly builds run on the page's event loop: tasks are
//! `spawn_local` futures, timers are `setTimeout`, and the clocks come from
//! `performance.now()` and `Date.now()`. A browser has one thread, so
//! `spawn_blocking` runs its closure as an ordinary task and panics abort the
//! module instead of unwinding. Code in the core spawns and sleeps through
//! this module, never through tokio directly.

/// `Send` where tasks may move between threads, nothing in the browser.
/// Bounds on spawned futures use it so the browser can spawn the fetch
/// futures that hold JavaScript values.
#[cfg(not(target_family = "wasm"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_family = "wasm"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_family = "wasm")]
pub trait MaybeSend {}
#[cfg(target_family = "wasm")]
impl<T> MaybeSend for T {}

/// `Sync` where values are shared between threads, nothing in the browser.
#[cfg(not(target_family = "wasm"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_family = "wasm"))]
impl<T: Sync> MaybeSync for T {}
#[cfg(target_family = "wasm")]
pub trait MaybeSync {}
#[cfg(target_family = "wasm")]
impl<T> MaybeSync for T {}

/// A boxed future that is `Send` wherever tasks are.
#[cfg(not(target_family = "wasm"))]
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;
#[cfg(target_family = "wasm")]
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + 'a>>;

#[cfg(not(target_family = "wasm"))]
pub use native::*;
#[cfg(target_family = "wasm")]
pub use web::*;

#[cfg(not(target_family = "wasm"))]
mod native {
    pub use std::time::{Instant, SystemTime, UNIX_EPOCH};
    pub use tokio::task::{spawn, spawn_blocking, JoinError, JoinHandle};
    pub use tokio::time::{error::Elapsed, interval, sleep, timeout, Interval};

    /// Resolve at `deadline`, or now if it has passed.
    pub async fn sleep_until(deadline: Instant) {
        tokio::time::sleep_until(deadline.into()).await;
    }

    /// Spawn from code that may run outside the runtime, such as a `Drop`
    /// during shutdown. Without a runtime the work is skipped.
    pub fn spawn_detached<F>(future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(future);
        }
    }
}

#[cfg(target_family = "wasm")]
mod web {
    use futures::future::{AbortHandle, Abortable, Either};
    use std::fmt;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::sync::oneshot;

    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

    /// A spawned task. Dropping it detaches the task, as tokio's does.
    pub struct JoinHandle<T> {
        output: oneshot::Receiver<T>,
        abort: AbortHandle,
    }

    impl<T> JoinHandle<T> {
        /// Stop the task at its next await point.
        pub fn abort(&self) {
            self.abort.abort();
        }

        /// Whether the task has stopped, by finishing or by being aborted.
        pub fn is_finished(&self) -> bool {
            self.abort.is_aborted() || self.output.is_terminated()
        }
    }

    impl<T> Future for JoinHandle<T> {
        type Output = Result<T, JoinError>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.output)
                .poll(cx)
                .map(|result| result.map_err(|_| JoinError))
        }
    }

    /// Why a task produced no output. In a browser that only happens when it
    /// was aborted: a panic aborts the whole module rather than one task.
    #[derive(Debug)]
    pub struct JoinError;

    impl JoinError {
        pub fn is_panic(&self) -> bool {
            false
        }

        pub fn is_cancelled(&self) -> bool {
            true
        }
    }

    impl fmt::Display for JoinError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("task was cancelled")
        }
    }

    impl std::error::Error for JoinError {}

    /// Run a future on the page's event loop.
    pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let (sender, output) = oneshot::channel();
        let (abort, registration) = AbortHandle::new_pair();
        let task = Abortable::new(future, registration);
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(value) = task.await {
                let _ = sender.send(value);
            }
        });
        JoinHandle { output, abort }
    }

    /// Spawn from code that may run outside a task, such as a `Drop`.
    pub fn spawn_detached<F: Future<Output = ()> + 'static>(future: F) {
        spawn(future);
    }

    /// Run blocking work as its own task. There are no other threads, so the
    /// work runs on the event loop once the caller yields.
    pub fn spawn_blocking<F, R>(work: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + 'static,
        R: 'static,
    {
        spawn(async move { work() })
    }

    /// Resolve after `duration`.
    pub async fn sleep(duration: Duration) {
        let millis = duration.as_millis().min(u32::MAX as u128) as u32;
        gloo_timers::future::TimeoutFuture::new(millis).await;
    }

    /// Resolve at `deadline`, or now if it has passed.
    pub async fn sleep_until(deadline: Instant) {
        sleep(deadline.saturating_duration_since(Instant::now())).await;
    }

    /// The deadline passed before the future finished.
    #[derive(Debug, PartialEq, Eq)]
    pub struct Elapsed;

    impl fmt::Display for Elapsed {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("deadline has elapsed")
        }
    }

    impl std::error::Error for Elapsed {}

    /// Run `future`, giving up after `duration`.
    pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Elapsed> {
        let deadline = std::pin::pin!(sleep(duration));
        let future = std::pin::pin!(future);
        match futures::future::select(future, deadline).await {
            Either::Left((value, _)) => Ok(value),
            Either::Right(_) => Err(Elapsed),
        }
    }

    /// A fixed-period ticker whose first tick completes immediately.
    pub struct Interval {
        period: Duration,
        next: Instant,
    }

    /// Tick every `period`, starting now.
    pub fn interval(period: Duration) -> Interval {
        Interval {
            period,
            next: Instant::now(),
        }
    }

    impl Interval {
        /// Wait for the next tick. Missed ticks are skipped, not replayed.
        pub async fn tick(&mut self) -> Instant {
            sleep_until(self.next).await;
            let now = Instant::now();
            self.next += self.period;
            if self.next <= now {
                self.next = now + self.period;
            }
            now
        }
    }
}
