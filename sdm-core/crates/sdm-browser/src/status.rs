//! `ChromeExtensionStatus.json`: the last version the extension reported and
//! when. The SwiftUI app reads the same file with Swift's default `Date`
//! decoding, so `lastSeen` is seconds since 2001-01-01 UTC.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "ChromeExtensionStatus.json";
/// Seconds from 1970-01-01 to Swift's reference date, 2001-01-01.
const SWIFT_REFERENCE_OFFSET: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionStatus {
    pub version: String,
    /// Seconds since 2001-01-01 UTC.
    pub last_seen: f64,
}

impl ExtensionStatus {
    pub fn last_seen_at(&self) -> SystemTime {
        let unix = (self.last_seen + SWIFT_REFERENCE_OFFSET).max(0.0);
        UNIX_EPOCH + std::time::Duration::from_secs_f64(unix)
    }
}

pub fn write_status(dir: &Path, version: &str, now: SystemTime) -> std::io::Result<()> {
    let unix = now
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    let status = ExtensionStatus {
        version: version.to_owned(),
        last_seen: unix - SWIFT_REFERENCE_OFFSET,
    };
    let json = serde_json::to_string_pretty(&status).map_err(std::io::Error::other)?;
    crate::cookies::write_cookie_file(&dir.join(FILE_NAME), &json)
}

pub fn read_status(dir: &Path) -> Option<ExtensionStatus> {
    let bytes = std::fs::read(dir.join(FILE_NAME)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
