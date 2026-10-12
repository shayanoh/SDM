use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use sdm_core::{RangeSet, SourceUrl};
use serde::{Deserialize, Serialize};

/// Resume state written alongside the `.incomplete` file, so the file and its
/// recovery information travel together. See spec §4.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSidecar {
    pub format_version: i64,
    #[serde(rename = "sourceURL")]
    pub source_url: SourceUrl,
    /// The URL the last successful probe/fetch actually resolved to, after
    /// redirects. A redirecting origin (e.g. a mirror-network frontend, like
    /// Fedora's `download.fedoraproject.org`) can send each connection to a
    /// different backend with its own `ETag`/`Last-Modified`; probing
    /// `source_url` fresh on every resume would then compare against whichever
    /// mirror happens to answer *this time*, not the one the bytes on disk
    /// actually came from, and `matches` would spuriously fail and discard a
    /// perfectly good partial download. Reusing the pinned mirror keeps the
    /// validator comparison meaningful across resumes. `None` for sidecars
    /// written before this field existed; those fall back to re-resolving
    /// `source_url`.
    #[serde(
        rename = "resolvedURL",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub resolved_url: Option<SourceUrl>,
    pub total_bytes: i64,
    /// `ETag` or `Last-Modified` captured when the download started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<String>,
    pub completed: RangeSet,
    /// The subset of `completed` that had been `fsync`ed when this sidecar
    /// was written. The file is only flushed to the device about once a
    /// minute (`DownloadTask::Configuration::sync_interval_ticks`), while the
    /// sidecar is rewritten far more often, so `completed` can be ahead of
    /// what would survive a power cut. `None` for sidecars written before
    /// this field existed — those were always flushed first, so their
    /// `completed` is itself durable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub durable_completed: Option<RangeSet>,
    /// `kern.bootsessionuuid` when this sidecar was written. Same value now
    /// means the machine has not restarted since, so everything written is
    /// still in the page cache (or on disk) and `completed` can be trusted
    /// whole. See [`ResumeSidecar::resumable_ranges`].
    #[serde(
        rename = "bootSessionID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub boot_session_id: Option<String>,
}

impl ResumeSidecar {
    pub const CURRENT_FORMAT_VERSION: i64 = 1;

    /// A sidecar at the current format version with no durable-range or
    /// boot-session information; the optional fields are plain `pub` fields.
    pub fn new(
        source_url: SourceUrl,
        resolved_url: Option<SourceUrl>,
        total_bytes: i64,
        validator: Option<String>,
        completed: RangeSet,
    ) -> Self {
        Self {
            format_version: Self::CURRENT_FORMAT_VERSION,
            source_url,
            resolved_url,
            total_bytes,
            validator,
            completed,
            durable_completed: None,
            boot_session_id: None,
        }
    }

    pub fn with_durable_completed(mut self, durable_completed: RangeSet) -> Self {
        self.durable_completed = Some(durable_completed);
        self
    }

    pub fn with_boot_session_id(mut self, boot_session_id: impl Into<String>) -> Self {
        self.boot_session_id = Some(boot_session_id.into());
        self
    }

    pub fn with_format_version(mut self, format_version: i64) -> Self {
        self.format_version = format_version;
        self
    }

    pub fn path_for(final_path: &Path) -> PathBuf {
        let mut path = final_path.as_os_str().to_owned();
        path.push(".sdmpart");
        PathBuf::from(path)
    }

    /// Whether the remote resource still matches what was captured at start.
    pub fn matches(&self, total_bytes: i64, validator: Option<&str>) -> bool {
        self.total_bytes == total_bytes && self.validator.as_deref() == validator
    }

    /// The ranges a resume may trust.
    ///
    /// Written bytes that were never `fsync`ed survive a crash or a force
    /// quit — the kernel still holds them — but not a power cut or kernel
    /// panic. A changed boot session is how the difference is told apart:
    /// same session, trust everything recorded; different session (or one
    /// that cannot be determined), fall back to what was flushed. Trusting
    /// `completed` after a power cut would stitch new bytes onto blocks
    /// that never reached the disk and finalize a silently corrupt file.
    pub fn resumable_ranges(&self, boot_session_id: Option<&str>) -> RangeSet {
        let Some(durable) = &self.durable_completed else {
            return self.completed.clone();
        };
        if let (Some(current), Some(recorded)) = (boot_session_id, self.boot_session_id.as_deref())
            && current == recorded
        {
            return self.completed.clone();
        }
        durable.clone()
    }

    /// This boot's `kern.bootsessionuuid`, or `None` if it cannot be read
    /// (always `None` off macOS). Read once and cached.
    pub fn current_boot_session_id() -> Option<String> {
        static VALUE: OnceLock<Option<String>> = OnceLock::new();
        VALUE.get_or_init(read_boot_session_id).clone()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let data = serde_json::to_vec(self).map_err(io::Error::other)?;
        write_atomically(path, &data)
    }

    /// Loads a sidecar, returning `None` when it is missing, unreadable,
    /// corrupt, or written by a newer format version.
    ///
    /// Deliberately non-throwing: an unusable sidecar always means "restart
    /// from zero", never "retry the load".
    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let sidecar: Self = serde_json::from_slice(&data).ok()?;
        (sidecar.format_version == Self::CURRENT_FORMAT_VERSION).then_some(sidecar)
    }

    pub fn remove(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(target_os = "macos")]
fn read_boot_session_id() -> Option<String> {
    let name = c"kern.bootsessionuuid";
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer asks sysctl for the required size only.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 || size == 0 {
        return None;
    }
    let mut buffer = vec![0u8; size];
    // SAFETY: `buffer` is `size` writable bytes; sysctl updates `size`.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    buffer.truncate(size);
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn read_boot_session_id() -> Option<String> {
    None
}

/// Replaces `path` with `data` atomically: writes a temporary file in the
/// same directory, syncs it, then renames it over the destination (what
/// Foundation's `Data.write(to:options: .atomic)` does; it calls plain
/// `fsync(2)` on the temporary file before the rename, so a power cut cannot
/// leave a zero-length destination). The sync is `fsync`, not `sync_all`,
/// which is the stronger `F_FULLFSYNC` on macOS. A failed write removes the
/// temporary file and leaves any existing destination untouched.
pub(crate) fn write_atomically(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let temp_path = path.with_file_name(temp_name);

    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o666) // the umask reduces this to the usual 0644
            .open(&temp_path)?;
        file.write_all(data)?;
        crate::sparse_file::fsync(&file)?;
        drop(file);
        std::fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}
