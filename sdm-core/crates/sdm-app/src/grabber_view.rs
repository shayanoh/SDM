//! Turns the grabber session's snapshot into the view the UI renders. Pure:
//! every text and rule the SwiftUI view derives lives here, not in the frontend.

use std::collections::HashSet;

use sdm_core::{FormatChoice, QualityPreferences, VideoCodec};
use sdm_grabber::{
    GrabberSnapshot, MediaFormatMenu, MediaRow, MediaRowState, ProbedLink, Stage, Verdict,
};

use crate::protocol::*;

pub(crate) fn build_view(
    snapshot: &GrabberSnapshot,
    preferences: &QualityPreferences,
    held_back: Option<u32>,
    tools: Option<ToolsNotice>,
) -> GrabberView {
    let mut rows: Vec<GrabberRow> = snapshot
        .links
        .iter()
        .map(|link| GrabberRow::Link(link_row(link)))
        .collect();
    rows.extend(
        snapshot
            .media_rows
            .iter()
            .map(|row| GrabberRow::Media(media_row(row, preferences))),
    );
    let packages = snapshot
        .packages
        .iter()
        .map(|package| {
            let wanted: HashSet<_> = package.link_ids.iter().copied().collect();
            let row_ids = snapshot
                .links
                .iter()
                .map(|l| l.id)
                .chain(snapshot.media_rows.iter().map(|r| r.id))
                .filter(|id| wanted.contains(id))
                .map(|id| id.to_string())
                .collect();
            GrabberPackage {
                id: package.id.to_string(),
                name: package.name.clone(),
                note: package.note.clone(),
                row_ids,
            }
        })
        .collect();
    GrabberView {
        checked_count: snapshot.checked_count as u32,
        total_count: snapshot.total_count as u32,
        summary: GrabberSummaryDto {
            online: snapshot.online_count() as u32,
            faulty: snapshot.faulty_count() as u32,
            offline: snapshot.offline_count() as u32,
            failed: snapshot.failed_count() as u32,
            is_checking: snapshot.is_checking(),
            recheckable: snapshot.recheckable_count() as u32,
        },
        rows,
        packages,
        held_back,
        tools,
    }
}

fn positive(bytes: Option<i64>) -> Option<f64> {
    bytes.filter(|b| *b > 0).map(|b| b as f64)
}

fn link_row(link: &ProbedLink) -> GrabberLinkRow {
    GrabberLinkRow {
        id: link.id.to_string(),
        filename: link.effective_filename(),
        final_url: link.final_url.as_str().to_owned(),
        stage: match link.stage {
            Stage::Queued => LinkStageDto::Queued,
            Stage::Probing => LinkStageDto::Probing,
            Stage::Sniffing => LinkStageDto::Sniffing,
            Stage::Done => LinkStageDto::Done,
        },
        verdict: link.verdict.as_ref().map(|verdict| match verdict {
            Verdict::Online => VerdictDto::Online,
            Verdict::Offline => VerdictDto::Offline,
            Verdict::CheckFailed => VerdictDto::CheckFailed,
            Verdict::Faulty { reason } => VerdictDto::Faulty {
                reason: reason.clone(),
            },
        }),
        size_bytes: positive(link.content_length),
        is_duplicate: link.is_duplicate,
        removable: link.verdict.is_some(),
    }
}

fn media_row(row: &MediaRow, preferences: &QualityPreferences) -> GrabberMediaRow {
    let (state, state_text) = match &row.state {
        MediaRowState::Resolving => (MediaStateDto::Resolving, None),
        MediaRowState::Resolved => (MediaStateDto::Resolved, None),
        MediaRowState::Unselected => (MediaStateDto::Unselected, None),
        MediaRowState::Unsupported => (MediaStateDto::Unsupported, None),
        MediaRowState::NeedsYtDlp => (MediaStateDto::NeedsYtDlp, None),
        MediaRowState::NeedsFfmpeg => (MediaStateDto::NeedsFfmpeg, None),
        MediaRowState::Failed(reason) => (MediaStateDto::Failed, Some(reason.clone())),
    };
    let can_pick = matches!(
        row.state,
        MediaRowState::Resolved | MediaRowState::Unselected | MediaRowState::NeedsFfmpeg
    );
    let options = match (&row.media, can_pick) {
        (Some(media), true) => {
            let all = MediaFormatMenu::options(media, preferences);
            let (matching, other): (Vec<_>, Vec<_>) =
                all.into_iter().partition(|o| o.matches_preferences);
            matching
                .into_iter()
                .chain(other)
                .map(|option| FormatOptionDto {
                    selected: row.choice.as_ref() == Some(&option.choice),
                    id: option.id,
                    label: option.label,
                    matches_preferences: option.matches_preferences,
                    is_wholesale: option.is_wholesale,
                })
                .collect()
        }
        _ => Vec::new(),
    };
    GrabberMediaRow {
        id: row.id.to_string(),
        title: row.title.clone(),
        source_url: row.source_url.as_str().to_owned(),
        state,
        state_text,
        summary: (row.state == MediaRowState::Resolved).then(|| summary_text(row.choice.as_ref())),
        options,
        is_duplicate: row.is_duplicate,
        size_bytes: positive(row.combined_bytes()),
    }
}

/// `Streaming · 1080p · avc1 · mp4`, as the SwiftUI picker label reads.
pub(crate) fn summary_text(choice: Option<&FormatChoice>) -> String {
    let Some(choice) = choice else {
        return "resolved".to_owned();
    };
    let mut parts = vec![if choice.wholesale_selector.is_some() {
        "Streaming".to_owned()
    } else {
        "Direct".to_owned()
    }];
    if let Some(video) = &choice.video {
        if let Some(height) = video.height {
            parts.push(format!("{height}p"));
        }
        if let Some(codec) = &video.vcodec {
            parts.push(codec_text(codec));
        }
    }
    parts.push(choice.output_container.file_extension().to_owned());
    parts.join(" · ")
}

fn codec_text(codec: &VideoCodec) -> String {
    match codec {
        VideoCodec::Av1 => "av1".to_owned(),
        VideoCodec::Vp9 => "vp9".to_owned(),
        VideoCodec::H264 => "h264".to_owned(),
        VideoCodec::Other(value) => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{
        MediaContainer, MediaDelivery, MediaFormat, MediaKind, ResolvedMedia, SourceUrl,
    };
    use sdm_grabber::PackageCandidate;

    fn url(text: &str) -> SourceUrl {
        SourceUrl::new(text).unwrap()
    }

    fn snapshot(
        links: Vec<ProbedLink>,
        media: Vec<MediaRow>,
        packages: Vec<PackageCandidate>,
    ) -> GrabberSnapshot {
        let total = links.len() + media.len();
        GrabberSnapshot::new(links, media, packages, 0, total)
    }

    fn view(snapshot: &GrabberSnapshot) -> GrabberView {
        build_view(snapshot, &QualityPreferences::default(), None, None)
    }

    fn progressive(id: &str, height: i64, delivery: MediaDelivery) -> MediaFormat {
        MediaFormat::new(
            id,
            MediaKind::Progressive,
            Some(height),
            None,
            Some(VideoCodec::H264),
            None,
            MediaContainer::Mp4,
            Some(1_000_000),
            None,
            None,
            url("https://cdn.example/v.mp4"),
        )
        .with_delivery(delivery)
    }

    fn resolved_row(title: &str) -> MediaRow {
        let media = ResolvedMedia::new(
            "youtube",
            "abc",
            title,
            None,
            vec![
                progressive("18", 360, MediaDelivery::Direct),
                progressive("22", 720, MediaDelivery::Direct),
            ],
        );
        MediaRow::new(url("https://youtu.be/abc"))
            .with_title(title)
            .with_state(MediaRowState::Unselected)
            .with_media(media)
    }

    #[test]
    fn rows_keep_session_order_links_then_media() {
        let a = ProbedLink::new(url("https://e.com/a.zip"));
        let m = MediaRow::new(url("https://youtu.be/x"));
        let view = view(&snapshot(vec![a.clone()], vec![m.clone()], vec![]));
        assert!(matches!(&view.rows[0], GrabberRow::Link(l) if l.id == a.id.to_string()));
        assert!(matches!(&view.rows[1], GrabberRow::Media(r) if r.id == m.id.to_string()));
    }

    #[test]
    fn package_row_ids_are_links_then_media_rows() {
        let l1 = ProbedLink::new(url("https://e.com/1.zip"));
        let l2 = ProbedLink::new(url("https://e.com/2.zip"));
        let m = MediaRow::new(url("https://youtu.be/x"));
        // The candidate lists the media row between the links.
        let package = PackageCandidate::new("pkg", vec![l1.id, m.id, l2.id]);
        let view = view(&snapshot(
            vec![l1.clone(), l2.clone()],
            vec![m.clone()],
            vec![package],
        ));
        assert_eq!(
            view.packages[0].row_ids,
            vec![l1.id.to_string(), l2.id.to_string(), m.id.to_string()]
        );
    }

    #[test]
    fn link_verdict_and_stage_map() {
        let queued = ProbedLink::new(url("https://e.com/q"));
        let online = ProbedLink::new(url("https://e.com/o"))
            .with_stage(Stage::Done)
            .with_verdict(Verdict::Online);
        let faulty = ProbedLink::new(url("https://e.com/f"))
            .with_stage(Stage::Done)
            .with_verdict(Verdict::Faulty {
                reason: "HTML page".into(),
            });
        let view = view(&snapshot(vec![queued, online, faulty], vec![], vec![]));
        let links: Vec<_> = view
            .rows
            .iter()
            .map(|r| match r {
                GrabberRow::Link(l) => l.clone(),
                _ => panic!(),
            })
            .collect();
        assert_eq!(links[0].stage, LinkStageDto::Queued);
        assert_eq!(links[0].verdict, None);
        assert_eq!(links[1].verdict, Some(VerdictDto::Online));
        assert_eq!(
            links[2].verdict,
            Some(VerdictDto::Faulty {
                reason: "HTML page".into()
            })
        );
    }

    #[test]
    fn removable_only_with_a_verdict() {
        let pending = ProbedLink::new(url("https://e.com/p"));
        let done = ProbedLink::new(url("https://e.com/d")).with_verdict(Verdict::Offline);
        let view = view(&snapshot(vec![pending, done], vec![], vec![]));
        let flags: Vec<bool> = view
            .rows
            .iter()
            .map(|r| matches!(r, GrabberRow::Link(l) if l.removable))
            .collect();
        assert_eq!(flags, vec![false, true]);
    }

    #[test]
    fn size_is_none_for_zero_or_missing_length() {
        let none = ProbedLink::new(url("https://e.com/a"));
        let zero = ProbedLink::new(url("https://e.com/b")).with_content_length(0);
        let some = ProbedLink::new(url("https://e.com/c")).with_content_length(1500);
        let view = view(&snapshot(vec![none, zero, some], vec![], vec![]));
        let sizes: Vec<Option<f64>> = view
            .rows
            .iter()
            .map(|r| match r {
                GrabberRow::Link(l) => l.size_bytes,
                _ => None,
            })
            .collect();
        assert_eq!(sizes, vec![None, None, Some(1500.0)]);
    }

    #[test]
    fn summary_text_matches_swiftui() {
        let direct = FormatChoice::new(
            Some(progressive("22", 720, MediaDelivery::Direct)),
            None,
            MediaContainer::Mp4,
            None,
        );
        assert_eq!(summary_text(Some(&direct)), "Direct · 720p · h264 · mp4");
        let streaming = direct.clone().with_wholesale_selector("22");
        assert_eq!(
            summary_text(Some(&streaming)),
            "Streaming · 720p · h264 · mp4"
        );
        assert_eq!(summary_text(None), "resolved");
    }

    #[test]
    fn options_only_for_pickable_states() {
        let mut resolving = resolved_row("t");
        resolving.state = MediaRowState::Resolving;
        let mut failed = resolved_row("t");
        failed.state = MediaRowState::Failed("boom\nmore".into());
        let unselected = resolved_row("t");
        let mut needs = resolved_row("t");
        needs.state = MediaRowState::NeedsFfmpeg;
        let view = view(&snapshot(
            vec![],
            vec![resolving, failed, unselected, needs],
            vec![],
        ));
        let counts: Vec<usize> = view
            .rows
            .iter()
            .map(|r| match r {
                GrabberRow::Media(m) => m.options.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(counts[0], 0);
        assert_eq!(counts[1], 0);
        assert!(counts[2] >= 2);
        assert!(counts[3] >= 2);
    }

    #[test]
    fn the_selected_option_is_the_rows_choice() {
        let mut row = resolved_row("t");
        let options =
            MediaFormatMenu::options(row.media.as_ref().unwrap(), &QualityPreferences::default());
        let picked = options.last().unwrap().clone();
        row.choice = Some(picked.choice.clone());
        row.state = MediaRowState::Resolved;
        let view = view(&snapshot(vec![], vec![row], vec![]));
        let GrabberRow::Media(media) = &view.rows[0] else {
            panic!()
        };
        let selected: Vec<&str> = media
            .options
            .iter()
            .filter(|o| o.selected)
            .map(|o| o.id.as_str())
            .collect();
        assert_eq!(selected, vec![picked.id.as_str()]);
        assert!(media.summary.as_deref().unwrap().starts_with("Direct"));
    }

    #[test]
    fn matching_options_come_before_non_matching() {
        let row = resolved_row("t");
        let mut prefs = QualityPreferences::default();
        prefs.max_height = 360;
        let view = build_view(&snapshot(vec![], vec![row], vec![]), &prefs, None, None);
        let GrabberRow::Media(media) = &view.rows[0] else {
            panic!()
        };
        let flags: Vec<bool> = media
            .options
            .iter()
            .map(|o| o.matches_preferences)
            .collect();
        let first_false = flags.iter().position(|f| !f).unwrap_or(flags.len());
        assert!(flags[first_false..].iter().all(|f| !f), "{flags:?}");
        assert!(flags.contains(&true) && flags.contains(&false), "{flags:?}");
    }

    #[test]
    fn a_failed_state_carries_the_full_reason() {
        let mut row = MediaRow::new(url("https://youtu.be/x"));
        row.state = MediaRowState::Failed("first line\nsecond".into());
        let view = view(&snapshot(vec![], vec![row], vec![]));
        let GrabberRow::Media(media) = &view.rows[0] else {
            panic!()
        };
        assert_eq!(media.state, MediaStateDto::Failed);
        assert_eq!(media.state_text.as_deref(), Some("first line\nsecond"));
    }

    #[test]
    fn summary_counts_and_the_checking_flag() {
        let online = ProbedLink::new(url("https://e.com/a"))
            .with_stage(Stage::Done)
            .with_verdict(Verdict::Online);
        let failed = ProbedLink::new(url("https://e.com/b"))
            .with_stage(Stage::Done)
            .with_verdict(Verdict::CheckFailed);
        let probing = ProbedLink::new(url("https://e.com/c")).with_stage(Stage::Probing);
        let view = view(&snapshot(vec![online, failed, probing], vec![], vec![]));
        assert_eq!(view.summary.online, 1);
        assert_eq!(view.summary.failed, 1);
        assert_eq!(view.summary.recheckable, 1);
        assert!(view.summary.is_checking);
    }
}
