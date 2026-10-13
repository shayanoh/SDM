use std::sync::Arc;

use sdm_core::{ByteRange, SourceUrl};
use uuid::Uuid;

use crate::file_signature::FileSignature;
use crate::probe_transport::{
    ProbeError, ProbeMethod, ProbeRequest, ProbeResponse, ProbeTransport,
};
use crate::probed_link::{ProbedLink, Stage};

/// Runs spec §7.2's stage 1 (cheap probe) and, when the link looks alive and
/// deep sniff is enabled, stage 2 (magic-byte sniff).
#[derive(Clone)]
pub struct LinkProber {
    transport: Arc<dyn ProbeTransport>,
    pub deep_sniff_enabled: bool,
}

impl LinkProber {
    /// Swift defaults `deepSniffEnabled` to `true`; pass it explicitly here.
    pub fn new(transport: Arc<dyn ProbeTransport>, deep_sniff_enabled: bool) -> Self {
        Self {
            transport,
            deep_sniff_enabled,
        }
    }

    /// Probes with a fresh id, for callers that don't need continuity with
    /// an existing record.
    pub async fn probe(&self, url: &SourceUrl) -> ProbedLink {
        self.probe_with_id(url, Uuid::new_v4()).await
    }

    /// `GrabberSession` passes its own tracking id explicitly so the
    /// returned `ProbedLink.id` matches the dictionary key it's merged back
    /// under — otherwise the link's own identity would silently diverge from
    /// the id everything else (snapshots, removal, manual overrides) uses to
    /// look it up.
    pub async fn probe_with_id(&self, url: &SourceUrl, id: Uuid) -> ProbedLink {
        let mut link = ProbedLink::new(url.clone())
            .with_id(id)
            .with_stage(Stage::Probing);

        match self.stage_one_response(url).await {
            Ok(response) => Self::apply(&response, &mut link),
            Err(_) => {
                link.transport_failed = true;
                link.stage = Stage::Done;
                return link;
            }
        }

        self.sniff_if_needed(&mut link).await;
        link.stage = Stage::Done;
        link
    }

    /// HEAD first; falls back to a `Range: bytes=0-0` GET when the origin
    /// rejects or lies about HEAD.
    async fn stage_one_response(&self, url: &SourceUrl) -> Result<ProbeResponse, ProbeError> {
        match self
            .transport
            .send(ProbeRequest::new(url.clone(), ProbeMethod::Head))
            .await
        {
            Ok(response) => Ok(response),
            Err(_) => {
                self.transport
                    .send(
                        ProbeRequest::new(url.clone(), ProbeMethod::Get)
                            .with_range(ByteRange::new(0, 1)),
                    )
                    .await
            }
        }
    }

    fn apply(response: &ProbeResponse, link: &mut ProbedLink) {
        link.final_url = response.final_url.clone();
        link.status_code = Some(response.status_code);
        link.accepts_ranges = response.status_code == 206;
        if let Some(length) = response
            .headers
            .get("content-length")
            .and_then(|text| text.parse::<i64>().ok())
        {
            link.content_length = Some(length);
        }
        if let Some(content_range) = response.headers.get("content-range")
            && let Some(slash) = content_range.rfind('/')
        {
            let total = &content_range[slash + 1..];
            if total != "*"
                && let Ok(value) = total.parse::<i64>()
            {
                link.content_length = Some(value);
            }
        }
        // Swift's `split` drops empty pieces, so `"; x"` yields `" x"`.
        link.content_type = response.headers.get("content-type").and_then(|value| {
            value
                .split(';')
                .find(|part| !part.is_empty())
                .map(str::to_owned)
        });
        link.validator = response
            .headers
            .get("etag")
            .or_else(|| response.headers.get("last-modified"))
            .cloned();
        link.suggested_filename =
            Self::filename_from_content_disposition(response.headers.get("content-disposition"));
    }

    fn filename_from_content_disposition(header: Option<&String>) -> Option<String> {
        let header = header?;
        for part in header.split(';').filter(|part| !part.is_empty()) {
            let trimmed = part.trim_matches(is_foundation_whitespace);
            if !trimmed.to_lowercase().starts_with("filename=") {
                continue;
            }
            let value: String = trimmed.chars().skip("filename=".chars().count()).collect();
            let value = value.trim_matches('"');
            // One path component only: a header must not pick the folder.
            return sdm_core::single_path_component(value);
        }
        None
    }

    async fn sniff_if_needed(&self, link: &mut ProbedLink) {
        let Some(status) = link.status_code else {
            return;
        };
        if !self.deep_sniff_enabled || !(200..=299).contains(&status) {
            return;
        }
        link.stage = Stage::Sniffing;
        let Ok(response) = self
            .transport
            .send(
                ProbeRequest::new(link.final_url.clone(), ProbeMethod::Get)
                    .with_range(ByteRange::new(0, 65536)),
            )
            .await
        else {
            return;
        };
        link.sniffed_signature = Some(FileSignature::detect(&response.body));
    }
}

/// Foundation's `CharacterSet.whitespaces`: the Unicode `Zs` category and
/// the tab (not the newlines, unlike `char::is_whitespace`).
fn is_foundation_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    )
}
