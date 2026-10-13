//! The one error type every fallible exported function returns.
//!
//! UniFFI turns a Rust panic into a thrown Swift error only for functions that
//! can throw: a non-throwing Swift binding calls the Rust function with
//! `try!`, so a panic in one would trap the whole app. Every exported function
//! therefore returns `Result<_, CoreError>`, and [`CoreError::Panicked`] is
//! what a panic becomes (see `runtime::guard` and `runtime::run`).

/// A failure the Rust core reports to Swift.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// The caller passed a value the core cannot accept (an empty package
    /// name, a zero segment count, a URL string that does not parse...). The
    /// Swift core would have trapped on its `precondition`; here it is a
    /// recoverable error.
    #[error("invalid argument `{argument}`: {message}")]
    InvalidArgument { argument: String, message: String },
    /// Rust code panicked inside the call. The call's effects are undefined,
    /// but the core stays usable: the engine, the grabber session and this
    /// crate's own locks all recover a poisoned lock rather than propagating
    /// it.
    #[error("the Rust core panicked: {message}")]
    Panicked { message: String },
    /// An operation failed for a reason outside the caller's control (the
    /// runtime shut down under the call, a file could not be created...).
    #[error("{message}")]
    Failed { message: String },
}

impl CoreError {
    pub(crate) fn invalid(argument: &str, message: impl Into<String>) -> Self {
        CoreError::InvalidArgument {
            argument: argument.to_owned(),
            message: message.into(),
        }
    }
}
