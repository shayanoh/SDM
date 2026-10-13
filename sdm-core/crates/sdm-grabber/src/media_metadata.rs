use sdm_core::{AudioCodec, FormatChoice, ResolvedMedia, VideoCodec};

/// Builds the one-line media description shown in the details panel for a
/// resolver-backed download: delivery, resolution, codecs, container,
/// bitrate, duration, and the source site. Pure. Parent spec
/// `2026-09-03-multi-site-resolver-design.md`.
pub enum MediaMetadata {}

impl MediaMetadata {
    pub fn describe(choice: &FormatChoice, media: &ResolvedMedia) -> String {
        let mut parts: Vec<String> = Vec::new();

        parts.push(
            if choice.is_wholesale() {
                "Streaming"
            } else {
                "Direct"
            }
            .into(),
        );

        if let Some(height) = choice.video.as_ref().and_then(|v| v.height) {
            parts.push(format!("{height}p"));
        }
        if let Some(vcodec) = choice.video.as_ref().and_then(|v| v.vcodec.as_ref()) {
            parts.push(describe_video(vcodec));
        }
        let acodec = choice
            .audio
            .as_ref()
            .and_then(|a| a.acodec.as_ref())
            .or_else(|| choice.video.as_ref().and_then(|v| v.acodec.as_ref()));
        if let Some(acodec) = acodec {
            parts.push(describe_audio(acodec));
        }

        parts.push(choice.output_container.file_extension().to_owned());

        if let Some(bitrate) = bitrate(choice) {
            parts.push(bitrate);
        }
        if let Some(duration) = duration(media.duration_seconds) {
            parts.push(duration);
        }
        let site = media.extractor.to_lowercase();
        if !site.is_empty() && site != "unknown" && site != "generic" {
            parts.push(site);
        }

        parts.join(" · ")
    }
}

/// yt-dlp `tbr` is in kbps; sum video + audio when muxing.
fn bitrate(choice: &FormatChoice) -> Option<String> {
    let total = choice.video.as_ref().and_then(|v| v.tbr).unwrap_or(0.0)
        + choice.audio.as_ref().and_then(|a| a.tbr).unwrap_or(0.0);
    if total.is_nan() || total <= 0.0 {
        return None;
    }
    if total >= 1000.0 {
        return Some(format!("{:.1} Mbps", total / 1000.0));
    }
    Some(format!("{} kbps", total.round() as i64))
}

fn duration(seconds: Option<f64>) -> Option<String> {
    let seconds = seconds.filter(|s| s.is_finite() && *s >= 1.0)?;
    let total = seconds.round() as i64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    Some(if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    })
}

pub(crate) fn describe_video(codec: &VideoCodec) -> String {
    match codec {
        VideoCodec::Av1 => "av1".into(),
        VideoCodec::Vp9 => "vp9".into(),
        VideoCodec::H264 => "h264".into(),
        VideoCodec::Other(raw) => raw.clone(),
    }
}

fn describe_audio(codec: &AudioCodec) -> String {
    match codec {
        AudioCodec::Opus => "opus".into(),
        AudioCodec::Aac => "aac".into(),
        AudioCodec::Other(raw) => raw.clone(),
    }
}
