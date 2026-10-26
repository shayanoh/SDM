use sdm_browser::framing::{FramingError, MAX_INCOMING, read_message, write_message};

#[test]
fn a_message_round_trips_with_a_little_endian_length() {
    let mut wire = Vec::new();
    write_message(&mut wire, br#"{"success":true}"#).unwrap();
    assert_eq!(&wire[..4], &16u32.to_le_bytes());
    let mut input = wire.as_slice();
    assert_eq!(read_message(&mut input).unwrap(), br#"{"success":true}"#);
}

#[test]
fn an_empty_stream_is_eof() {
    let mut input: &[u8] = &[];
    assert_eq!(read_message(&mut input), Err(FramingError::Eof));
}

#[test]
fn a_zero_length_message_is_refused() {
    let mut input: &[u8] = &0u32.to_le_bytes();
    assert_eq!(read_message(&mut input), Err(FramingError::ZeroLength));
}

#[test]
fn a_message_over_the_cap_is_refused_before_it_is_read() {
    let mut wire = ((MAX_INCOMING + 1) as u32).to_le_bytes().to_vec();
    wire.extend_from_slice(b"x");
    let mut input = wire.as_slice();
    assert_eq!(read_message(&mut input), Err(FramingError::TooLarge));
}

#[test]
fn a_truncated_body_is_eof() {
    let mut wire = 10u32.to_le_bytes().to_vec();
    wire.extend_from_slice(b"short");
    let mut input = wire.as_slice();
    assert_eq!(read_message(&mut input), Err(FramingError::Eof));
}

#[test]
fn a_reply_over_one_megabyte_is_refused() {
    let big = vec![b'x'; 1024 * 1024 + 1];
    let mut wire = Vec::new();
    assert_eq!(write_message(&mut wire, &big), Err(FramingError::TooLarge));
    assert!(wire.is_empty());
}
