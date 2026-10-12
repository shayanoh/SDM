use sdm_core::WholesalePhase;
use sdm_resolve::WholesaleProgressParser as Parser;

#[test]
fn parses_byte_progress() {
    let p = Parser::parse("sdm:downloading|1048576|10485760|NA|NA|NA").unwrap();
    assert_eq!(p.downloaded_bytes, Some(1_048_576));
    assert_eq!(p.total_bytes, Some(10_485_760));
    assert_eq!(p.phase, WholesalePhase::Downloading);
}

#[test]
fn promotes_estimate_when_total_missing() {
    let p = Parser::parse("sdm:downloading|500|NA|2000|NA|NA").unwrap();
    assert_eq!(p.total_bytes, Some(2000));
}

#[test]
fn parses_float_formatted_estimate() {
    // yt-dlp emits total_bytes_estimate as a float for HLS/DASH downloads.
    let p = Parser::parse("sdm:downloading|20420736|NA|13161769424.0|2|1318").unwrap();
    assert_eq!(p.downloaded_bytes, Some(20_420_736));
    assert_eq!(p.total_bytes, Some(13_161_769_424));
}

#[test]
fn derives_fraction_from_fragment_index_and_count() {
    let p = Parser::parse("sdm:downloading|1000|NA|NA|659|1318").unwrap();
    assert_eq!(p.fraction, Some(0.5));
    // A fragment count means yt-dlp's native (resumable) downloader is in use.
    assert!(p.is_fragmented);
}

#[test]
fn fragment_fraction_is_nil_without_fragment_info() {
    let p = Parser::parse("sdm:downloading|1000|4000|NA|NA|NA").unwrap();
    assert_eq!(p.fraction, None);
    assert_eq!(p.total_bytes, Some(4000));
    // No fragment count — not (yet) known to be the resumable downloader.
    assert!(!p.is_fragmented);
}

#[test]
fn handles_none_and_comma_grouped_numbers() {
    let p = Parser::parse("sdm:downloading|1,048,576|None|None|None|None").unwrap();
    assert_eq!(p.downloaded_bytes, Some(1_048_576));
    assert_eq!(p.total_bytes, None);
    assert_eq!(p.fraction, None);
}

#[test]
fn recognizes_post_processing() {
    assert_eq!(
        Parser::parse("[Merger] Merging formats into \"x.mp4\"").map(|p| p.phase),
        Some(WholesalePhase::PostProcessing)
    );
    assert_eq!(
        Parser::parse("[FixupM3u8] Fixing MPEG-TS in MP4 container of \"x.mp4\"").map(|p| p.phase),
        Some(WholesalePhase::PostProcessing)
    );
}

#[test]
fn ignores_unrelated_lines() {
    assert_eq!(Parser::parse("[youtube] Extracting URL: https://x"), None);
    assert_eq!(Parser::parse(""), None);
    assert_eq!(Parser::parse("sdm:downloading|only|three"), None);
}
