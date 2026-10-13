use sdm_core::SourceUrl;
use uuid::Uuid;

use crate::file_signature::FileSignature;
use crate::url_text;
use crate::verdict::Verdict;

/// Where a link is in the two-stage probing pipeline. Spec §7.5: rows appear
/// immediately as `Queued` and fill in as probes land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Queued,
    Probing,
    Sniffing,
    Done,
}

/// One link's progress through the two-stage probing pipeline. URLs keep the
/// string the user pasted ([`SourceUrl`]), so that string is what the
/// grabber shows, hands to the engine and de-duplicates on.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbedLink {
    pub id: Uuid,
    pub original_url: SourceUrl,
    pub final_url: SourceUrl,
    pub stage: Stage,
    pub status_code: Option<i64>,
    pub content_length: Option<i64>,
    pub content_type: Option<String>,
    pub suggested_filename: Option<String>,
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    pub validator: Option<String>,
    /// Whether a `206` came back — spec §7.2's resume-capable flag.
    pub accepts_ranges: bool,
    pub sniffed_signature: Option<FileSignature>,
    /// True when both the HEAD and the ranged-GET fallback threw — a DNS or
    /// timeout failure rather than an HTTP-level error status.
    pub transport_failed: bool,
    pub verdict: Option<Verdict>,
    pub is_duplicate: bool,
}

impl ProbedLink {
    /// A `Queued` link with a fresh id, `final_url == original_url` and
    /// nothing probed yet. The other Swift initializer arguments are the
    /// `with_*` builders (or plain field assignment).
    pub fn new(original_url: SourceUrl) -> Self {
        Self {
            id: Uuid::new_v4(),
            final_url: original_url.clone(),
            original_url,
            stage: Stage::Queued,
            status_code: None,
            content_length: None,
            content_type: None,
            suggested_filename: None,
            validator: None,
            accepts_ranges: false,
            sniffed_signature: None,
            transport_failed: false,
            verdict: None,
            is_duplicate: false,
        }
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_final_url(mut self, final_url: SourceUrl) -> Self {
        self.final_url = final_url;
        self
    }

    pub fn with_stage(mut self, stage: Stage) -> Self {
        self.stage = stage;
        self
    }

    pub fn with_status_code(mut self, status_code: i64) -> Self {
        self.status_code = Some(status_code);
        self
    }

    pub fn with_content_length(mut self, content_length: i64) -> Self {
        self.content_length = Some(content_length);
        self
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    pub fn with_suggested_filename(mut self, suggested_filename: impl Into<String>) -> Self {
        self.suggested_filename = Some(suggested_filename.into());
        self
    }

    pub fn with_validator(mut self, validator: impl Into<String>) -> Self {
        self.validator = Some(validator.into());
        self
    }

    pub fn with_accepts_ranges(mut self, accepts_ranges: bool) -> Self {
        self.accepts_ranges = accepts_ranges;
        self
    }

    pub fn with_sniffed_signature(mut self, signature: FileSignature) -> Self {
        self.sniffed_signature = Some(signature);
        self
    }

    pub fn with_transport_failed(mut self, transport_failed: bool) -> Self {
        self.transport_failed = transport_failed;
        self
    }

    pub fn with_verdict(mut self, verdict: Verdict) -> Self {
        self.verdict = Some(verdict);
        self
    }

    pub fn with_duplicate(mut self, is_duplicate: bool) -> Self {
        self.is_duplicate = is_duplicate;
        self
    }

    /// The name used for clustering and display: the server-declared name
    /// when Stage 1 captured one, otherwise the last URL path component.
    pub fn effective_filename(&self) -> String {
        // Whatever the source, the result is one path component: it becomes a
        // filename joined onto the download folder.
        if let Some(name) = self
            .suggested_filename
            .as_deref()
            .and_then(sdm_core::single_path_component)
        {
            return name;
        }
        let last = url_text::last_path_component(&self.final_url);
        sdm_core::safe_path_component(&last)
    }
}
