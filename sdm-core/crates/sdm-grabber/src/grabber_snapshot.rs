use crate::media_row::{MediaRow, MediaRowState};
use crate::package_clustering::PackageCandidate;
use crate::probed_link::{ProbedLink, Stage};
use crate::verdict::Verdict;

/// An immutable view of grabber state, published to the UI. Spec §7.5's
/// determinate `checked / total` header and verdict filter chips read
/// straight off this.
#[derive(Debug, Clone, PartialEq)]
pub struct GrabberSnapshot {
    pub links: Vec<ProbedLink>,
    pub media_rows: Vec<MediaRow>,
    pub packages: Vec<PackageCandidate>,
    pub checked_count: usize,
    pub total_count: usize,
}

impl GrabberSnapshot {
    pub fn new(
        links: Vec<ProbedLink>,
        media_rows: Vec<MediaRow>,
        packages: Vec<PackageCandidate>,
        checked_count: usize,
        total_count: usize,
    ) -> Self {
        Self {
            links,
            media_rows,
            packages,
            checked_count,
            total_count,
        }
    }

    pub fn online_count(&self) -> usize {
        self.count_verdict(|v| *v == Verdict::Online)
    }

    pub fn offline_count(&self) -> usize {
        self.count_verdict(|v| *v == Verdict::Offline)
    }

    pub fn faulty_count(&self) -> usize {
        self.count_verdict(|v| matches!(v, Verdict::Faulty { .. }))
    }

    pub fn failed_count(&self) -> usize {
        self.count_verdict(|v| *v == Verdict::CheckFailed)
    }

    /// True while any link is still queued/probing/sniffing or any media row
    /// is resolving — drives the header's "Recheck All" ⇄ "Cancel Checks"
    /// toggle.
    pub fn is_checking(&self) -> bool {
        self.links.iter().any(|l| l.stage != Stage::Done)
            || self
                .media_rows
                .iter()
                .any(|r| r.state == MediaRowState::Resolving)
    }

    /// Rows a "Recheck All" would actually retry: failed HTTP probes plus
    /// media rows that failed to resolve or need a binary.
    pub fn recheckable_count(&self) -> usize {
        self.failed_count()
            + self
                .media_rows
                .iter()
                .filter(|r| {
                    matches!(
                        r.state,
                        MediaRowState::Failed(_)
                            | MediaRowState::NeedsYtDlp
                            | MediaRowState::NeedsFfmpeg
                    )
                })
                .count()
    }

    fn count_verdict(&self, predicate: impl Fn(&Verdict) -> bool) -> usize {
        self.links
            .iter()
            .filter(|l| l.verdict.as_ref().is_some_and(&predicate))
            .count()
    }
}
