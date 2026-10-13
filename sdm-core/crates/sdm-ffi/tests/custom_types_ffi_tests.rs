//! The custom types, through the converters UniFFI generates (the same
//! `Lower` / `Lift` the scaffolding calls), plus the plain conversion
//! functions behind them.

use std::path::PathBuf;

use sdm_core::{ByteRange, RangeSet, SourceUrl};
use sdm_ffi::CoreError;
use sdm_ffi::UniFfiTag;
use sdm_ffi::custom_types::{
    RangeSetData, path_from_string, range_set_from_data, range_set_to_data, source_url_from_string,
    usize_from_u64, uuid_from_string,
};
use uniffi::{Lift, Lower};
use uuid::Uuid;

fn round_trip<T>(value: T) -> T
where
    T: Lower<UniFfiTag, FfiType = uniffi::RustBuffer>
        + Lift<UniFfiTag, FfiType = uniffi::RustBuffer>,
{
    let lowered = <T as Lower<UniFfiTag>>::lower(value);
    <T as Lift<UniFfiTag>>::try_lift(lowered).expect("lift what was lowered")
}

fn buffer_of_string(text: &str) -> uniffi::RustBuffer {
    <String as Lower<UniFfiTag>>::lower(text.to_owned())
}

/// URLs `url::Url` would rewrite: the string Swift handed over must come
/// back exactly, and must still compare equal to itself.
#[test]
fn source_url_keeps_the_exact_string_across_the_boundary() {
    for text in [
        "HTTP://Example.COM:80/a b/ü?#",
        "https://example.com/path with spaces/%7efile.zip",
        "https://example.com./trailing-dot",
        "https://EXAMPLE.com:443/x?",
        "https://example.com/日本語/ファイル.bin",
        "https://example.com/a/../b/./c.bin",
    ] {
        let original = source_url_from_string(text).expect(text);
        let back = round_trip(original.clone());
        assert_eq!(back.as_str(), text, "the string must survive byte for byte");
        assert_eq!(back, original, "and still de-duplicate against itself");
    }
}

#[test]
fn source_url_refuses_what_the_core_could_not_probe() {
    for text in [
        "",
        "not a url",
        "example.com/no-scheme",
        "http://exa mple.com/",
    ] {
        assert!(
            matches!(
                source_url_from_string(text),
                Err(CoreError::InvalidArgument { .. })
            ),
            "{text:?}"
        );
        let lifted = <SourceUrl as Lift<UniFfiTag>>::try_lift(buffer_of_string(text));
        let error = lifted.expect_err(text);
        assert!(
            error.downcast_ref::<CoreError>().is_some(),
            "UniFFI must be able to throw this as a CoreError: {error:?}"
        );
    }
}

#[test]
fn uuid_round_trips_and_accepts_swifts_upper_case() {
    let id = Uuid::new_v4();
    assert_eq!(round_trip(id), id);
    assert_eq!(
        uuid_from_string(&id.to_string().to_uppercase()).unwrap(),
        id
    );
    assert!(matches!(
        uuid_from_string("not-a-uuid"),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert!(<Uuid as Lift<UniFfiTag>>::try_lift(buffer_of_string("zzz")).is_err());
}

#[test]
fn path_round_trips_and_refuses_an_empty_path() {
    let path = PathBuf::from("/tmp/some dir/file.bin");
    assert_eq!(round_trip(path.clone()), path);
    assert!(matches!(
        path_from_string(""),
        Err(CoreError::InvalidArgument { .. })
    ));
}

#[test]
fn usize_crosses_as_a_sixty_four_bit_integer() {
    assert_eq!(round_trip_usize(0), 0);
    assert_eq!(round_trip_usize(12_345), 12_345);
    assert_eq!(usize_from_u64(7).unwrap(), 7);
}

fn round_trip_usize(value: usize) -> usize {
    let lowered = <usize as Lower<UniFfiTag>>::lower(value);
    <usize as Lift<UniFfiTag>>::try_lift(lowered).unwrap()
}

#[test]
fn range_set_round_trips_normalized() {
    let mut set = RangeSet::new();
    set.insert(ByteRange::new(0, 10));
    set.insert(ByteRange::new(20, 30));
    assert_eq!(round_trip(set.clone()), set);

    // Whatever Swift sends is merged and sorted again.
    let messy = RangeSetData {
        ranges: vec![
            ByteRange::new(20, 30),
            ByteRange::new(0, 12),
            ByteRange::new(10, 15),
            ByteRange::new(5, 5),
        ],
    };
    let rebuilt = range_set_from_data(&messy).unwrap();
    assert_eq!(
        rebuilt.ranges(),
        [ByteRange::new(0, 15), ByteRange::new(20, 30)]
    );
    assert_eq!(range_set_to_data(&rebuilt).ranges, rebuilt.ranges());
}

#[test]
fn range_set_refuses_an_inverted_or_negative_range() {
    for bad in [(-1, 5), (10, 2)] {
        // `ByteRange`'s fields are public, so Swift can build one the
        // constructor would reject.
        let data = RangeSetData {
            ranges: vec![ByteRange {
                start: bad.0,
                end: bad.1,
            }],
        };
        assert!(
            matches!(
                range_set_from_data(&data),
                Err(CoreError::InvalidArgument { .. })
            ),
            "{bad:?}"
        );
    }
}

/// The Swift conversion's stand-in for an address Foundation rejects must
/// not come back in as a real link, through the converter nor any export.
#[test]
fn the_unusable_url_placeholder_is_refused_when_lifted() {
    for text in [
        sdm_ffi::custom_types::UNUSABLE_URL_PLACEHOLDER,
        "about:invalid",
        "ABOUT:blank",
        "about:invalid#x",
    ] {
        assert!(
            matches!(
                source_url_from_string(text),
                Err(CoreError::InvalidArgument { .. })
            ),
            "{text}"
        );
        assert!(
            <SourceUrl as Lift<UniFfiTag>>::try_lift(buffer_of_string(text)).is_err(),
            "{text}"
        );
    }
    assert!(source_url_from_string("https://example.com/about:invalid").is_ok());
}
