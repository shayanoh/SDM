mod support;

use sdm_core::{
    Assembly, AudioCodec, ComponentOrigin, FormatChoice, MediaContainer, MediaDelivery,
    MediaFormat, MediaKind, ResolvedMedia, VideoCodec,
};
use sdm_grabber::{MediaHandoff, MediaRow, MediaRowState, ProbedLink, Stage, Verdict};
use support::{af, su, vf};

fn resolved_row(mux: bool) -> MediaRow {
    let v = vf("137", 1080, VideoCodec::H264, MediaContainer::Mp4);
    let a = af("140", AudioCodec::Aac, MediaContainer::M4a);
    let mut row = MediaRow::new(su("https://youtu.be/abc"));
    row.media = Some(ResolvedMedia::new(
        "youtube",
        "abc",
        "Clip",
        Some(10.0),
        vec![v.clone(), a.clone()],
    ));
    row.choice = Some(if mux {
        FormatChoice::new(Some(v), Some(a), MediaContainer::Mp4, Some(1100))
    } else {
        FormatChoice::new(Some(v), None, MediaContainer::Mp4, Some(1000))
    });
    row.state = MediaRowState::Resolved;
    row
}

fn done_link(url: &str, content_length: i64) -> ProbedLink {
    let mut link = ProbedLink::new(su(url)).with_stage(Stage::Done);
    link.status_code = Some(200);
    link.content_length = Some(content_length);
    link.verdict = Some(Verdict::Online);
    link
}

#[test]
fn a_mux_row_becomes_a_two_component_item() {
    let (items, held) = MediaHandoff::build(&[], &[resolved_row(true)]);
    assert_eq!(held, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].components.len(), 2);
    assert_eq!(items[0].assembly, Assembly::Mux);
    assert_eq!(items[0].output_filename, "Clip [abc].mp4");
    // The item's URL is the grabbed YouTube URL, not a googlevideo stream.
    assert_eq!(items[0].url(), &su("https://youtu.be/abc"));
    assert!(
        items[0].components[0]
            .url
            .as_str()
            .starts_with("https://gv/")
    );
    match &items[0].components[0].origin {
        ComponentOrigin::Resolved { format_id } => assert_eq!(format_id, "137"),
        other => panic!("component 0 should be .resolved, got {other:?}"),
    }
    assert!(items[0].components[1].part_filename.contains(".f140."));
}

#[test]
fn a_progressive_row_becomes_a_one_component_item_named_its_final_name() {
    let (items, _) = MediaHandoff::build(&[], &[resolved_row(false)]);
    assert_eq!(items[0].components.len(), 1);
    assert_eq!(items[0].assembly, Assembly::None);
    // No `.fNNN` for a single stream — the part file is the output file, so
    // nothing has to rename it and the item's destination matches disk.
    assert_eq!(
        items[0].components[0].part_filename,
        items[0].output_filename
    );
    assert_eq!(items[0].output_filename, "Clip [abc].mp4");
}

#[test]
fn a_wholesale_row_becomes_one_non_resumable_component() {
    let hls_fmt = MediaFormat::new(
        "270",
        MediaKind::VideoOnly,
        Some(1080),
        None,
        Some(VideoCodec::H264),
        None,
        MediaContainer::Mp4,
        None,
        None,
        None,
        su("https://x/m.m3u8"),
    )
    .with_delivery(MediaDelivery::Hls);
    let mut row = MediaRow::new(su("https://www.tiktok.com/@a/video/1"));
    row.media = Some(ResolvedMedia::new(
        "tiktok",
        "1",
        "Clip",
        Some(10.0),
        vec![hls_fmt.clone()],
    ));
    row.choice = Some(
        FormatChoice::new(Some(hls_fmt), None, MediaContainer::Mp4, None)
            .with_wholesale_selector("bv*+ba/b"),
    );
    row.state = MediaRowState::Resolved;

    let (items, held) = MediaHandoff::build(&[], &[row]);
    assert_eq!(held, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].components.len(), 1);
    assert_eq!(items[0].assembly, Assembly::None);
    assert_eq!(items[0].components[0].is_resumable, Some(false));
    assert_eq!(
        items[0].components[0].part_filename,
        items[0].output_filename
    );
    assert_eq!(items[0].url(), &su("https://www.tiktok.com/@a/video/1"));
    match &items[0].components[0].origin {
        ComponentOrigin::Wholesale { format_selector } => {
            assert_eq!(format_selector, "bv*+ba/b")
        }
        other => panic!("expected .wholesale origin, got {other:?}"),
    }
}

#[test]
fn media_items_carry_a_media_info_string() {
    let (items, _) = MediaHandoff::build(&[], &[resolved_row(true)]);
    assert!(
        items[0]
            .metadata
            .as_deref()
            .is_some_and(|m| m.starts_with("Direct · 1080p · h264 · aac · mp4"))
    );
}

#[test]
fn http_items_carry_release_tags_from_their_filename() {
    let tagged = done_link("https://x/The.Show.S01E01.1080p.WEB-DL.H.264-GRP.mkv", 5000);
    let plain = done_link("https://x/archive.zip", 100);
    let (items, _) = MediaHandoff::build(&[tagged, plain], &[]);
    assert_eq!(items[0].metadata.as_deref(), Some("1080p · WEB-DL · H.264"));
    assert_eq!(items[1].metadata, None);
}

#[test]
fn unselected_rows_are_held_back_but_http_siblings_go_through() {
    let mut unselected = MediaRow::new(su("https://youtu.be/x"));
    unselected.state = MediaRowState::Unselected;
    let http = done_link("https://example.com/f.zip", 500);
    let (items, held) = MediaHandoff::build(&[http], &[unselected]);
    assert_eq!(held, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].components[0].origin, ComponentOrigin::Http);
}
