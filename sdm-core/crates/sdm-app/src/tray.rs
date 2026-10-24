//! What the tray shows and what quitting asks, as pure functions of the
//! engine snapshot. The Tauri shell draws the icon, menu and popover from
//! this. Reference: the SwiftUI `MenuBarPopoverView`, `MenuBarIconController`
//! and `applicationShouldTerminate`.

use sdm_core::ItemState;
use sdm_engine::EngineSnapshot;
use serde::Serialize;
use specta::Type;

/// The title of the quit warning dialog.
pub const QUIT_TITLE: &str = "Quit with active downloads in progress?";

/// How many running downloads the tray lists before "and N more".
const LISTED: usize = 5;

/// One running download, as the tray shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrayItem {
    pub name: String,
    /// 0 to 1.
    pub fraction: f64,
    pub bytes_per_second: f64,
}

/// Everything the tray icon, menu, popover and quit guard read.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrayStatus {
    pub running_count: u32,
    /// Running downloads that are known not to be resumable.
    pub non_resumable_count: u32,
    pub bytes_per_second: f64,
    pub history: Vec<f64>,
    pub overall_fraction: f64,
    /// Every running download.
    pub items: Vec<TrayItem>,
    /// Grabbed links waiting in the Linkgrabber.
    pub links_waiting: u32,
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn unit_fraction(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl TrayStatus {
    pub fn from_snapshot(snapshot: &EngineSnapshot, links_waiting: u32) -> Self {
        let running: Vec<_> = snapshot
            .packages
            .iter()
            .flat_map(|package| package.items.iter())
            .filter(|item| item.state == ItemState::Running)
            .collect();
        Self {
            running_count: count(running.len()),
            non_resumable_count: count(
                running
                    .iter()
                    .filter(|item| item.is_resumable == Some(false))
                    .count(),
            ),
            bytes_per_second: snapshot.global_bytes_per_second,
            history: snapshot.global_history.clone(),
            overall_fraction: unit_fraction(snapshot.figures.overall_fraction),
            items: running
                .iter()
                .map(|item| TrayItem {
                    name: item.filename.clone(),
                    fraction: unit_fraction(item.fraction_completed()),
                    bytes_per_second: item.bytes_per_second,
                })
                .collect(),
            links_waiting,
        }
    }

    /// The text of the quit warning, or `None` when nothing is running.
    pub fn quit_warning(&self) -> Option<String> {
        let n = self.running_count;
        if n == 0 {
            return None;
        }
        let mut text = if n == 1 {
            "1 download is running. Quitting stops it; it resumes the next time SDM starts."
                .to_owned()
        } else {
            format!(
                "{n} downloads are running. Quitting stops them; they resume the next time SDM starts."
            )
        };
        match self.non_resumable_count {
            0 => {}
            1 => text.push_str(
                "\n\n1 of them cannot be resumed. Quitting now will lose its progress permanently.",
            ),
            k => text.push_str(&format!(
                "\n\n{k} of them cannot be resumed. Quitting now will lose their progress permanently."
            )),
        }
        Some(text)
    }

    /// The disabled status lines under the Linux tray menu.
    pub fn menu_status_lines(&self) -> Vec<String> {
        let n = self.running_count as usize;
        if n == 0 {
            return vec!["No downloads running".to_owned()];
        }
        let noun = if n == 1 { "download" } else { "downloads" };
        let mut lines = vec![format!(
            "{n} {noun} running · {}",
            format_speed(self.bytes_per_second)
        )];
        for item in self.items.iter().take(LISTED) {
            lines.push(format!(
                "{} — {}% — {}",
                item.name,
                (item.fraction * 100.0).floor() as u32,
                format_speed(item.bytes_per_second)
            ));
        }
        if n > LISTED {
            lines.push(format!("and {} more", n - LISTED));
        }
        lines
    }
}

/// Binary units with one decimal: `0 B/s`, `900 B/s`, `1.5 KB/s`, `12.4 MB/s`.
pub fn format_speed(bytes_per_second: f64) -> String {
    let value = if bytes_per_second.is_finite() {
        bytes_per_second.max(0.0)
    } else {
        0.0
    };
    if value < 1024.0 {
        return format!("{} B/s", value.round() as u64);
    }
    let mut scaled = value / 1024.0;
    for unit in ["KB/s", "MB/s", "GB/s"] {
        if scaled < 1024.0 || unit == "GB/s" {
            return format!("{scaled:.1} {unit}");
        }
        scaled /= 1024.0;
    }
    unreachable!("the loop returns at GB/s")
}

/// Decides when the tray icon must be redrawn: only when the picture would
/// look different. A port of `MenuBarIconController.update`.
#[derive(Debug, Default)]
pub struct IconTracker {
    drawn: Option<(bool, f64)>,
}

impl IconTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether to redraw for this state; remembers it when so. The ring moves
    /// by at least one pixel of the icon's width or it is the same picture.
    pub fn needs_redraw(&mut self, running: bool, fraction: f64, width_px: u32) -> bool {
        let fraction = unit_fraction(fraction);
        let one_pixel = 1.0 / f64::from(width_px.max(1));
        let redraw = match self.drawn {
            None => true,
            Some((was_running, _)) if was_running != running => true,
            Some((_, drawn)) => running && (fraction - drawn).abs() >= one_pixel,
        };
        if redraw {
            self.drawn = Some((running, fraction));
        }
        redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{EngineSnapshot, ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    const MB: f64 = 1024.0 * 1024.0;

    fn running(
        name: &str,
        done: i64,
        total: i64,
        speed: f64,
        resumable: Option<bool>,
    ) -> ItemSnapshot {
        let mut completed = RangeSet::new();
        if done > 0 {
            completed.insert(ByteRange::new(0, done));
        }
        ItemSnapshot::new(
            Uuid::new_v4(),
            SourceUrl::new("https://e.com/x").unwrap(),
            name,
            Some(total),
            completed,
            ItemState::Running,
            true,
            resumable,
            1,
            1,
            speed,
            vec![],
        )
    }

    fn snapshot(items: Vec<ItemSnapshot>) -> EngineSnapshot {
        let package = PackageSnapshot::new(Uuid::new_v4(), "P", Priority::Normal, items, 0.0, 1);
        EngineSnapshot::new(vec![package], 5.0 * MB, vec![1.0, 2.0], 1)
    }

    fn status(items: Vec<ItemSnapshot>) -> TrayStatus {
        TrayStatus::from_snapshot(&snapshot(items), 0)
    }

    fn many(n: usize) -> Vec<ItemSnapshot> {
        (0..n)
            .map(|i| running(&format!("file{i}.bin"), 50, 100, 1.0 * MB, Some(true)))
            .collect()
    }

    #[test]
    fn the_status_lists_every_running_item_with_its_figures() {
        let s = TrayStatus::from_snapshot(
            &snapshot(vec![running("a.iso", 25, 100, 2.0 * MB, Some(true))]),
            4,
        );
        assert_eq!(s.running_count, 1);
        assert_eq!(s.non_resumable_count, 0);
        assert_eq!(s.links_waiting, 4);
        assert_eq!(s.bytes_per_second, 5.0 * MB);
        assert_eq!(s.history, vec![1.0, 2.0]);
        assert_eq!(
            s.items,
            vec![TrayItem {
                name: "a.iso".into(),
                fraction: 0.25,
                bytes_per_second: 2.0 * MB
            }]
        );
    }

    #[test]
    fn quit_warning_is_none_when_nothing_runs() {
        assert_eq!(status(vec![]).quit_warning(), None);
    }

    #[test]
    fn quit_warning_for_resumable_downloads_has_one_paragraph() {
        let warning = status(many(3)).quit_warning().unwrap();
        assert_eq!(
            warning,
            "3 downloads are running. Quitting stops them; they resume the next time SDM starts."
        );
        let one = status(many(1)).quit_warning().unwrap();
        assert!(one.starts_with("1 download is running. Quitting stops it;"));
    }

    #[test]
    fn quit_warning_adds_the_non_resumable_sentence() {
        let mut items = many(2);
        items.push(running("c.bin", 1, 100, 0.0, Some(false)));
        let warning = status(items).quit_warning().unwrap();
        assert!(warning.starts_with("3 downloads are running."));
        assert!(warning.ends_with(
            "\n\n1 of them cannot be resumed. Quitting now will lose its progress permanently."
        ));
        let mut two = many(1);
        two.push(running("c.bin", 1, 100, 0.0, Some(false)));
        two.push(running("d.bin", 1, 100, 0.0, Some(false)));
        assert!(status(two).quit_warning().unwrap().ends_with(
            "2 of them cannot be resumed. Quitting now will lose their progress permanently."
        ));
    }

    #[test]
    fn an_unprobed_download_counts_as_running_but_not_as_non_resumable() {
        let s = status(vec![running("a.bin", 1, 100, 0.0, None)]);
        assert_eq!(s.running_count, 1);
        assert_eq!(s.non_resumable_count, 0);
        assert!(!s.quit_warning().unwrap().contains("cannot be resumed"));
    }

    #[test]
    fn menu_lines_for_zero_one_five_and_nine_running() {
        assert_eq!(
            status(vec![]).menu_status_lines(),
            vec!["No downloads running"]
        );

        let one = status(vec![running("a.iso", 25, 100, 2.0 * MB, Some(true))]);
        assert_eq!(
            one.menu_status_lines(),
            vec!["1 download running · 5.0 MB/s", "a.iso — 25% — 2.0 MB/s"]
        );

        let five = status(many(5)).menu_status_lines();
        assert_eq!(five.len(), 6);
        assert_eq!(five[0], "5 downloads running · 5.0 MB/s");

        let nine = status(many(9)).menu_status_lines();
        assert_eq!(nine.len(), 7);
        assert_eq!(nine[6], "and 4 more");
    }

    #[test]
    fn speeds_use_binary_units_with_one_decimal() {
        assert_eq!(format_speed(0.0), "0 B/s");
        assert_eq!(format_speed(900.0), "900 B/s");
        assert_eq!(format_speed(1536.0), "1.5 KB/s");
        assert_eq!(format_speed(12.4 * MB), "12.4 MB/s");
        assert_eq!(format_speed(f64::NAN), "0 B/s");
    }

    #[test]
    fn the_icon_redraws_only_when_the_picture_would_change() {
        let mut tracker = IconTracker::new();
        assert!(tracker.needs_redraw(false, 0.0, 28), "the first draw");
        assert!(!tracker.needs_redraw(false, 0.0, 28));
        assert!(
            !tracker.needs_redraw(false, 0.9, 28),
            "idle ignores progress"
        );
        assert!(tracker.needs_redraw(true, 0.0, 28), "running starts");
        assert!(!tracker.needs_redraw(true, 0.01, 28), "under one pixel");
        assert!(tracker.needs_redraw(true, 0.04, 28), "past one pixel");
        assert!(
            !tracker.needs_redraw(true, 0.05, 28),
            "measured from the drawn value"
        );
        assert!(tracker.needs_redraw(false, 0.0, 28), "running stops");
        assert!(
            tracker.needs_redraw(true, f64::NAN, 28),
            "NaN is zero progress"
        );
        assert!(!tracker.needs_redraw(true, 0.0, 28));
    }
}
