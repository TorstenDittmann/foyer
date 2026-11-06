//! A Tokio runtime next to GPUI's own executor.
//!
//! Network and disk work (reqwest needs Tokio) runs here; views await the
//! returned `JoinHandle`s from GPUI tasks, which works on any executor.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("foyer-io")
            .enable_all()
            .build()
            .expect("failed to start the I/O runtime")
    })
}

/// Run `future` on the I/O runtime.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}

/// Run blocking file work on the I/O runtime's blocking pool.
pub fn spawn_blocking<F, R>(f: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    runtime().spawn_blocking(f)
}
