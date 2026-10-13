use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterableLink {
    pub id: Uuid,
    pub filename: String,
    pub host: String,
    pub directory_path: String,
}

impl ClusterableLink {
    pub fn new(
        id: Uuid,
        filename: impl Into<String>,
        host: impl Into<String>,
        directory_path: impl Into<String>,
    ) -> Self {
        Self {
            id,
            filename: filename.into(),
            host: host.into(),
            directory_path: directory_path.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageCandidate {
    /// Stable identity, independent of `name` — two packages can legitimately
    /// share a name for a moment (see `GrabberSession.recluster()`'s
    /// duplicate-name merge), and even when they don't, `name` alone is not
    /// enough for a caller to say "act on *this* package" unambiguously.
    /// `GrabberSession` preserves this id across reclustering for whichever
    /// previous package had the same name, so it stays stable across a mere
    /// regrouping rather than becoming a new id every tick.
    pub id: Uuid,
    pub name: String,
    pub link_ids: Vec<Uuid>,
    pub is_archive: bool,
    /// An optional line shown under the package name — a truncated playlist's
    /// "50 of 320 videos". Carried through to `DownloadPackage.note` on
    /// handoff. Parent spec §5.4.
    pub note: Option<String>,
}

impl PackageCandidate {
    /// A non-archive candidate with a fresh id and no note.
    pub fn new(name: impl Into<String>, link_ids: Vec<Uuid>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            link_ids,
            is_archive: false,
            note: None,
        }
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_archive(mut self, is_archive: bool) -> Self {
        self.is_archive = is_archive;
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// Characters trimmed from package names (Swift `CharacterSet(charactersIn: " ._-")`).
const NAME_TRIM: [char; 4] = [' ', '.', '_', '-'];

/// A string that never shows up in a file name.
const SEASON_PLACEHOLDER: &str = "THESEASONNUMBERPLACEHOLDERFORCOMSHAYANOHSDM";

static SEASON_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^.*\.s(\d{2})e\d{2}\..*$").expect("valid regex"));

static ARCHIVE_PATTERNS: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [r"\.part\d+\.rar$", r"\.r\d\d$", r"\.z\d\d$", r"\.\d{3}$"]
        .map(|p| Regex::new(p).expect("valid regex"))
});

static SEASON_EPISODE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(.*?)(s\d{1,2})e\d{1,2}.*$").expect("valid regex"));

/// A pure function `[ClusterableLink] -> [PackageCandidate]`. Spec §7.4.
pub enum PackageClustering {}

impl PackageClustering {
    pub fn cluster(links: &[ClusterableLink]) -> Vec<PackageCandidate> {
        if links.is_empty() {
            return Vec::new();
        }
        let mut input_order: HashMap<Uuid, usize> = HashMap::new();
        for (index, link) in links.iter().enumerate() {
            input_order.entry(link.id).or_insert(index);
        }

        let mut archive_groups: HashMap<String, Vec<&ClusterableLink>> = HashMap::new();
        let mut remaining: Vec<&ClusterableLink> = Vec::new();
        for candidate in links {
            match archive_base_name(&candidate.filename) {
                Some(base) => archive_groups.entry(base).or_default().push(candidate),
                None => remaining.push(candidate),
            }
        }

        let mut template_groups: HashMap<String, Vec<&ClusterableLink>> = HashMap::new();
        for candidate in remaining {
            template_groups
                .entry(template(&candidate.filename))
                .or_default()
                .push(candidate);
        }

        let mut candidates: Vec<PackageCandidate> = Vec::new();
        let mut singletons: Vec<&ClusterableLink> = Vec::new();
        for (key, members) in &template_groups {
            if members.len() > 1 || key.starts_with("**SHOW**:") {
                candidates.push(PackageCandidate::new(
                    name(members),
                    members.iter().map(|m| m.id).collect(),
                ));
            } else {
                singletons.extend(members.iter().copied());
            }
        }

        // Cluster singletons by beautified filename
        for candidate in singletons {
            candidates.push(PackageCandidate::new(
                name(&[candidate]),
                vec![candidate.id],
            ));
        }

        for (base, members) in &archive_groups {
            let cleaned = base.trim_matches(&NAME_TRIM[..]);
            candidates.push(
                PackageCandidate::new(
                    if cleaned.is_empty() {
                        name(members)
                    } else {
                        beautify(cleaned)
                    },
                    members.iter().map(|m| m.id).collect(),
                )
                .with_archive(true),
            );
        }

        // Hash-map iteration order is randomized per process; sort by each
        // candidate's earliest-appearing member so output is deterministic
        // across calls and test runs.
        candidates.sort_by_key(|candidate| {
            candidate
                .link_ids
                .iter()
                .filter_map(|id| input_order.get(id).copied())
                .min()
                .unwrap_or(usize::MAX)
        });
        candidates
    }
}

/// Lowercased, extension stripped, separators normalized, digit runs
/// collapsed to `#` so `Show.S01E01.mkv` and `Show.S01E02.mkv` reduce to
/// the same template and cluster with no episode-specific regex. Also
/// we keep the season number intact so different seasons of a show
/// will have different templates.
/// Also will mark show-like filenames with a marker "**SHOW**:" to hint
/// the clusterer about namings.
fn template(filename: &str) -> String {
    let mut stem = strip_extension(filename).to_lowercase();

    // We don't want S01E01 to be parsed to S#E# but S01E#, the only
    // way is to replace 01 with some placeholder, and replace it back at the end
    let mut season_previous_value = String::new();
    if let Some(range) = SEASON_PATTERN
        .captures(&stem)
        .and_then(|captures| captures.get(1))
        .map(|m| m.range())
    {
        season_previous_value = stem[range.clone()].to_owned();
        stem.replace_range(range, SEASON_PLACEHOLDER);
    }

    // Swift iterates `Character`s (grapheme clusters) and asks the first
    // scalar's properties, so a letter keeps its combining marks.
    let mut result = String::new();
    let mut last_was_digit = false;
    for grapheme in stem.graphemes(true) {
        let first = grapheme.chars().next().unwrap_or(' ');
        if first.is_alphabetic() || first.is_numeric() {
            if first.is_numeric() {
                if !last_was_digit {
                    result.push('#');
                }
                last_was_digit = true;
            } else {
                result.push_str(grapheme);
                last_was_digit = false;
            }
        } else {
            result.push(' ');
            last_was_digit = false;
        }
    }
    result = result.replace(SEASON_PLACEHOLDER, &season_previous_value);
    if !season_previous_value.is_empty() {
        result = format!("**SHOW**:{result}");
    }
    result
        .split(' ')
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Detects `.part01.rar`, `.r00`, `.z01`, and `.001`-style archive
/// parts, returning the shared base name that locks them into one
/// package regardless of template.
fn archive_base_name(filename: &str) -> Option<String> {
    let lower = filename.to_lowercase();
    ARCHIVE_PATTERNS
        .iter()
        .find_map(|regex| regex.find(&lower))
        .map(|m| lower[..m.start()].to_owned())
}

/// Swift `URL(fileURLWithPath: filename).deletingPathExtension().lastPathComponent`.
fn strip_extension(filename: &str) -> String {
    let trimmed = filename.trim_end_matches('/');
    let component = trimmed.rsplit('/').next().unwrap_or("");
    match component.rfind('.') {
        Some(dot) if dot > 0 && dot + 1 < component.len() => component[..dot].to_owned(),
        _ => component.to_owned(),
    }
}

/// Cleaned longest common prefix of member stems, falling back to the
/// host. Tries the season/episode-aware prefix first, since a naive
/// character-by-character common prefix mangles that specific pattern.
fn name(members: &[&ClusterableLink]) -> String {
    let stems: Vec<String> = members
        .iter()
        .map(|m| strip_extension(&m.filename))
        .collect();
    let Some(first) = stems.first() else {
        return "Package".into();
    };

    if let Some(season_prefix) = season_episode_prefix(&stems) {
        return beautify(&season_prefix);
    }

    let mut prefix = first.clone();
    for stem in stems.iter().skip(1) {
        prefix = common_prefix(&prefix, stem);
        if prefix.is_empty() {
            break;
        }
    }
    let cleaned = prefix.trim_matches(&NAME_TRIM[..]);
    if cleaned.is_empty() {
        return members
            .first()
            .map_or_else(|| "Package".into(), |m| m.host.clone());
    }
    beautify(cleaned)
}

fn common_prefix(a: &str, b: &str) -> String {
    let mut result = String::new();
    for (grapheme_a, grapheme_b) in a.graphemes(true).zip(b.graphemes(true)) {
        if grapheme_a != grapheme_b {
            break;
        }
        result.push_str(grapheme_a);
    }
    result
}

/// Detects a shared "S01E0x" style season/episode structure across
/// every member's stem and, when found, returns the name up through the
/// season token — "Show.S01E01" and "Show.S01E02" name as "Show.S01",
/// not the meaningless "Show.S01E0" a naive character-by-character
/// common prefix produces, since that stops the instant the episode
/// digits diverge, mid-token.
fn season_episode_prefix(stems: &[String]) -> Option<String> {
    let mut shared: Option<(String, String)> = None;
    for stem in stems {
        let captures = SEASON_EPISODE_PATTERN.captures(stem)?;
        let leading = captures.get(1)?.as_str().to_owned();
        let season = captures.get(2)?.as_str().to_uppercase();
        match &shared {
            Some((current_leading, current_season)) => {
                if current_leading.to_lowercase() != leading.to_lowercase()
                    || *current_season != season
                {
                    return None;
                }
            }
            None => shared = Some((leading, season)),
        }
    }
    shared.map(|(leading, season)| leading + &season)
}

/// Replaces `.`/`_`/`-` separators with spaces and collapses runs of
/// them, so a package name reads like a title instead of a filename.
/// Only capitalizes words that are entirely lowercase — "1080p",
/// "BluRay", and "NASA" are left exactly as the source spelled them,
/// since re-casing an already-mixed-case or all-caps word is as likely
/// to mangle it as improve it.
fn beautify(raw: &str) -> String {
    raw.split(['.', '_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            if word != word.to_lowercase() {
                return word.to_owned();
            }
            let mut graphemes = word.graphemes(true);
            match graphemes.next() {
                Some(head) => head.to_uppercase() + graphemes.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
