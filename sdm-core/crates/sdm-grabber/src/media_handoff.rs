use sdm_core::{Assembly, ComponentOrigin, DownloadItem, FileComponent, ItemState};

use crate::media_metadata::MediaMetadata;
use crate::media_row::{MediaRow, MediaRowState};
use crate::probed_link::ProbedLink;
use crate::release_tags::ReleaseTags;

/// Pure translation of a package's grabber rows into engine
/// `DownloadItem`s. HTTP links become one-component items; a `Resolved`
/// media row becomes a 1–2-component item. Rows with no usable format are
/// held back. Parent spec §6.3.
pub enum MediaHandoff {}

impl MediaHandoff {
    /// Returns the items and how many media rows were held back.
    pub fn build(http_links: &[ProbedLink], media_rows: &[MediaRow]) -> (Vec<DownloadItem>, usize) {
        let mut items: Vec<DownloadItem> = Vec::new();
        let mut held_back = 0;

        for link in http_links {
            let effective = link.effective_filename();
            let filename = if effective.is_empty() {
                "download".to_owned()
            } else {
                effective
            };
            let mut item = DownloadItem::http(link.original_url.clone(), filename.clone());
            if let Some(length) = link.content_length.filter(|length| *length > 0) {
                item.set_total_bytes(Some(length));
            }
            item.metadata = ReleaseTags::extract(&filename);
            items.push(item);
        }

        for row in media_rows {
            let (MediaRowState::Resolved, Some(media), Some(choice)) =
                (&row.state, &row.media, &row.choice)
            else {
                held_back += 1;
                continue;
            };
            let output = row.display_filename();
            let stem = format!("{} [{}]", MediaRow::sanitize(&media.title), media.video_id);
            let media_info = MediaMetadata::describe(choice, media);
            let mut components: Vec<FileComponent> = Vec::new();

            // HLS/DASH-only: one non-resumable component that yt-dlp
            // downloads wholesale against the page URL. Spec §6.4.
            if let Some(selector) = &choice.wholesale_selector {
                let mut component = FileComponent::new(
                    row.source_url.clone(),
                    format!("{stem}.{}", choice.output_container.file_extension()),
                )
                .with_origin(ComponentOrigin::Wholesale {
                    format_selector: selector.clone(),
                })
                .with_is_resumable(Some(false));
                component.total_bytes = choice.estimated_bytes;
                component.part_filename = output.clone();
                items.push(
                    DownloadItem::new(vec![component], output)
                        .with_source_url(row.source_url.clone())
                        .with_assembly(Assembly::None)
                        .with_state(ItemState::Queued)
                        .with_metadata(media_info),
                );
                continue;
            }

            for format in [&choice.video, &choice.audio].into_iter().flatten() {
                let mut component = FileComponent::new(
                    format.url.clone(),
                    format!(
                        "{stem}.f{}.{}",
                        format.id,
                        format.container.file_extension()
                    ),
                )
                .with_origin(ComponentOrigin::Resolved {
                    format_id: format.id.clone(),
                });
                component.total_bytes = format.filesize_effective();
                components.push(component);
            }
            if components.is_empty() {
                held_back += 1;
                continue;
            }
            // A single progressive/audio-only stream needs no assembly — name
            // its part file the final name from the start so nothing has to
            // rename it. Only a mux keeps `.fNNN` names (two parts share the
            // folder until ffmpeg combines them).
            if components.len() == 1 {
                components[0].part_filename = output.clone();
            }
            let assembly = if components.len() == 2 {
                Assembly::Mux
            } else {
                Assembly::None
            };
            items.push(
                // The grabbed YouTube URL, not the googlevideo stream —
                // this is what the details panel shows.
                DownloadItem::new(components, output)
                    .with_source_url(row.source_url.clone())
                    .with_assembly(assembly)
                    .with_state(ItemState::Queued)
                    .with_metadata(media_info),
            );
        }

        (items, held_back)
    }
}
