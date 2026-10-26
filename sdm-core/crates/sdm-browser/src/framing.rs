//! Chrome's native-messaging framing: a 32-bit little-endian length, then that
//! many bytes of JSON.

use std::io::{Read, Write};

/// Chrome allows far more, but a cookie snapshot is the largest message.
pub const MAX_INCOMING: usize = 8 * 1024 * 1024;
/// Chrome's limit for a message from the host.
pub const MAX_OUTGOING: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramingError {
    Eof,
    ZeroLength,
    TooLarge,
    Io(String),
}

pub fn read_message(input: &mut impl Read) -> Result<Vec<u8>, FramingError> {
    let mut length = [0u8; 4];
    read_exactly(input, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 {
        return Err(FramingError::ZeroLength);
    }
    if length > MAX_INCOMING {
        return Err(FramingError::TooLarge);
    }
    let mut body = vec![0u8; length];
    read_exactly(input, &mut body)?;
    Ok(body)
}

pub fn write_message(output: &mut impl Write, payload: &[u8]) -> Result<(), FramingError> {
    if payload.len() > MAX_OUTGOING {
        return Err(FramingError::TooLarge);
    }
    let io = |e: std::io::Error| FramingError::Io(e.to_string());
    output
        .write_all(&(payload.len() as u32).to_le_bytes())
        .map_err(io)?;
    output.write_all(payload).map_err(io)?;
    output.flush().map_err(io)
}

fn read_exactly(input: &mut impl Read, buffer: &mut [u8]) -> Result<(), FramingError> {
    input
        .read_exact(buffer)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::UnexpectedEof => FramingError::Eof,
            _ => FramingError::Io(error.to_string()),
        })
}
