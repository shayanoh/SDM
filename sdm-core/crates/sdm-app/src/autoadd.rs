//! Which grabbed packages are ready to go straight to downloads. A port of
//! the SwiftUI app's auto-add-and-start rule (`SDMApp.swift`).

use std::collections::HashSet;

use sdm_grabber::{GrabberSnapshot, MediaRowState, Verdict};
use uuid::Uuid;

/// A package is ready when it has rows, none of them was added before, every
/// link is online and every media row is resolved. A half-ready package
/// waits. Returns `(package id, its row ids)` in session order.
pub(crate) fn ready_packages(
    snapshot: &GrabberSnapshot,
    already: &HashSet<Uuid>,
) -> Vec<(Uuid, Vec<Uuid>)> {
    snapshot
        .packages
        .iter()
        .filter(|package| {
            let ids: HashSet<Uuid> = package.link_ids.iter().copied().collect();
            if ids.is_empty() || !ids.is_disjoint(already) {
                return false;
            }
            let mut links = snapshot
                .links
                .iter()
                .filter(|l| ids.contains(&l.id))
                .peekable();
            let mut media = snapshot
                .media_rows
                .iter()
                .filter(|r| ids.contains(&r.id))
                .peekable();
            (links.peek().is_some() || media.peek().is_some())
                && links.all(|l| l.verdict == Some(Verdict::Online))
                && media.all(|r| r.state == MediaRowState::Resolved)
        })
        .map(|package| (package.id, package.link_ids.clone()))
        .collect()
}
