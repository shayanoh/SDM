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

/// Empty input is empty output, as in Swift. The output buffer is reserved up
/// front from the compressed size (the stream does not record the total), so
/// inflating a multi-megabyte binary does not repeatedly regrow and copy it.
pub fn decode(input: &[u8]) -> Result<Vec<u8>, LzfseError> {
    decode_with_size_hint(input, input.len().saturating_mul(OUTPUT_SIZE_RATIO))
}

/// Typical expansion of a compressed executable, used for the reservation.
const OUTPUT_SIZE_RATIO: usize = 3;

/// Upper bound for the reservation, so a hostile or odd input cannot ask for
/// an absurd allocation up front (the vector still grows past it if needed).
const MAX_SIZE_HINT: usize = 512 * 1024 * 1024;

/// [`decode`] with the expected output size, when the caller knows it.
pub fn decode_with_size_hint(input: &[u8], expected_len: usize) -> Result<Vec<u8>, LzfseError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    output
        .try_reserve(expected_len.min(MAX_SIZE_HINT))
        .map_err(|_| LzfseError)?;
    lzfse_rust::decode_bytes(input, &mut output).map_err(|_| LzfseError)?;
    Ok(output)
}
