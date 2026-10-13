//! The one tokio runtime every exported call runs on, and the two helpers
//! that keep a Rust panic from crossing into Swift as an abort.

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

use crate::error::CoreError;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

/// The process-wide multi-thread runtime, created on first use.
///
/// Exported `async fn`s do not use UniFFI's `async_runtime = "tokio"`
/// (which would run them on `async_compat`'s own hidden runtime): each one
/// hands its work to this runtime with [`run`] and awaits the join handle, so
/// the engine's jobs, the grabber's probes and `reqwest`'s connections all
/// live on the runtime this crate owns.
///
/// # Panics
///
/// Panics if the operating system refuses to create the runtime's threads.
pub fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .enable_all()
            .thread_name("sdm-core")
            .build()
            .expect("could not start the SDM core runtime")
    })
}

/// A panic payload as text.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with a non-text payload".to_owned()
    }
}

/// Runs a synchronous body with the runtime entered (the grabber's
/// non-`async` methods may `tokio::spawn`) and turns a panic into
/// [`CoreError::Panicked`].
pub(crate) fn guard<T>(body: impl FnOnce() -> T) -> Result<T, CoreError> {
    let _entered = runtime().enter();
    catch_unwind(AssertUnwindSafe(body)).map_err(|payload| CoreError::Panicked {
        message: panic_message(payload.as_ref()),
    })
}

/// Runs `work` on the core runtime and waits for it.
///
/// A panic inside `work` becomes [`CoreError::Panicked`]. Dropping the
/// returned future (a cancelled Swift `Task`) detaches the spawned task
/// instead of cancelling it, so a half-finished `add` or `remove_item` still
/// completes: engine operations are not meant to be abandoned midway.
pub(crate) async fn run<T, F>(work: F) -> Result<T, CoreError>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    match runtime().spawn(work).await {
        Ok(value) => Ok(value),
        Err(error) if error.is_panic() => Err(CoreError::Panicked {
            message: panic_message(error.into_panic().as_ref()),
        }),
        Err(_) => Err(CoreError::Failed {
            message: "the core runtime shut down before the call finished".to_owned(),
        }),
    }
}
