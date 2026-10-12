//! LZFSE, used to ship the bundled `ffmpeg` / `qjs` binaries compressed
//! inside the `.app` and inflate them into the managed `bin/` directory on
//! first run.
//!
//! The Swift twin wraps `Compression`'s LZFSE stream; this is the pure-Rust
//! `lzfse_rust` crate, which reads what macOS writes (pinned by
//! `decodes_a_file_compressed_by_macos`).

/// Twin of Swift `LZFSEError.failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("failed")]
pub struct LzfseError;

/// Empty input is empty output, as in Swift.
pub fn encode(input: &[u8]) -> Result<Vec<u8>, LzfseError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    lzfse_rust::encode_bytes(input, &mut output).map_err(|_| LzfseError)?;
    Ok(output)
}

/// Empty input is empty output, as in Swift.
pub fn decode(input: &[u8]) -> Result<Vec<u8>, LzfseError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    lzfse_rust::decode_bytes(input, &mut output).map_err(|_| LzfseError)?;
    Ok(output)
}
