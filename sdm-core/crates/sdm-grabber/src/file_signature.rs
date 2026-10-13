/// A magic-byte identification of a small header sample. Spec §7.2 stage 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSignature {
    Zip,
    Rar,
    Gzip,
    Mp4,
    Mkv,
    Pdf,
    Html,
    Unknown,
}

impl FileSignature {
    pub fn detect(data: &[u8]) -> FileSignature {
        let bytes = &data[..data.len().min(16)];
        if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04])
            || bytes.starts_with(&[0x50, 0x4B, 0x05, 0x06])
        {
            return FileSignature::Zip;
        }
        if bytes.starts_with(&[0x52, 0x61, 0x72, 0x21]) {
            return FileSignature::Rar;
        }
        if bytes.starts_with(&[0x1F, 0x8B]) {
            return FileSignature::Gzip;
        }
        if bytes.len() >= 8
            && bytes[4] == 0x66
            && bytes[5] == 0x74
            && bytes[6] == 0x79
            && bytes[7] == 0x70
        {
            return FileSignature::Mp4;
        }
        if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return FileSignature::Mkv;
        }
        if bytes.starts_with(&[0x25, 0x50, 0x44, 0x46]) {
            return FileSignature::Pdf;
        }
        // Like Swift's `String(data:encoding:)`, a 512-byte prefix that is
        // not valid UTF-8 (including one cut mid-character) is not text.
        if let Ok(text) = std::str::from_utf8(&data[..data.len().min(512)]) {
            let lowered = text.to_lowercase();
            if lowered.contains("<!doctype html") || lowered.contains("<html") {
                return FileSignature::Html;
            }
        }
        FileSignature::Unknown
    }

    /// Whether this signature is consistent with a claimed file extension.
    /// `Unknown` never contradicts — there was not enough data to sniff, so
    /// it must not itself trigger a "faulty" verdict.
    pub fn matches_extension(&self, ext: &str) -> bool {
        let ext = ext.to_lowercase();
        matches!(
            (self, ext.as_str()),
            (FileSignature::Unknown, _)
                | (FileSignature::Zip, "zip")
                | (FileSignature::Rar, "rar")
                | (FileSignature::Gzip, "gz" | "tgz")
                | (FileSignature::Mp4, "mp4" | "m4v")
                | (FileSignature::Mkv, "mkv")
                | (FileSignature::Pdf, "pdf")
        )
    }
}
