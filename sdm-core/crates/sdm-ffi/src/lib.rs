//! The UniFFI surface of the Rust core: a static library and a dynamic
//! library the Swift package links.
//!
//! * `engine`, `grabber` and `binaries` hold the three exported objects.
//! * `records` declares the domain types of the four library crates to
//!   UniFFI; `custom_types` maps `Uuid`, `SourceUrl`, `PathBuf`, `RangeSet`
//!   and `usize` onto Foundation types.
//! * `functions` holds the pure helpers the views call.
//! * `runtime` owns the one tokio runtime and keeps panics from crossing the
//!   boundary as aborts; `error` is the error every function returns.
//!
//! The Swift side of each custom type is configured in `uniffi.toml`.

uniffi::setup_scaffolding!();

pub mod binaries;
pub mod custom_types;
pub mod engine;
pub mod error;
pub mod functions;
pub mod grabber;
pub mod records;
pub mod runtime;
pub mod shared;

pub use binaries::{ManagedBinaries, ManagedBinariesConfig};
pub use engine::{DownloadEngine, EngineConfig, EngineDependencies};
pub use error::CoreError;
pub use functions::*;
pub use grabber::{GrabberConfig, GrabberSession};
