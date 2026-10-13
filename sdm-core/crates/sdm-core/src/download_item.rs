use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::byte_range::ByteRange;
use crate::file_component::{Assembly, ComponentOrigin, FileComponent};
use crate::priority::{ItemState, Priority};
use crate::range_set::RangeSet;
use crate::serde_support::uuid_upper;
use crate::source_url::SourceUrl;

/// One download. Serializes in the nested shape (`components`,
/// `outputFilename`, `sourceURL`, ...); deserializes both that and the
/// legacy flat shape (`url`, `filename`, `totalBytes`, `completed`, ...).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    #[serde(with = "uuid_upper")]
    pub id: Uuid,
    /// Invariant: at least one. A generic HTTP download has exactly one;
    /// a muxed YouTube download has two (video + audio). Parent spec §5.
    pub components: Vec<FileComponent>,
    /// The final file once assembly (if any) completes.
    pub output_filename: String,
    /// The user-facing origin of the download: the URL that was grabbed.
    /// For a generic HTTP download this equals the (single) component's URL;
    /// for a YouTube item it is the `watch?v=` / `youtu.be` URL, while each
    /// component's own `url` is the direct `googlevideo` stream.
    #[serde(rename = "sourceURL")]
    pub source_url: SourceUrl,
    pub assembly: Assembly,
    pub state: ItemState,
    /// Purely user-managed: "never start this, no matter what." Independent
    /// of `state` — see `ItemState`. Only Disable/Enable changes it.
    pub is_enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    /// Position within the owning package. Lower sorts earlier.
    pub position: i64,
    /// A human-readable one-liner describing the media: for a resolver-backed
    /// item its delivery / resolution / codecs / container / site; for a
    /// plain HTTP item the release tags mined from its filename (`1080p ·
    /// WEB-DL · H.264`). `None` / empty ⇒ the UI shows a dash. Filled once at
    /// grab time by `MediaHandoff`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
}

impl DownloadItem {
    /// An item over `components` with a fresh id, `source_url` taken from
    /// the first component, `Assembly::None`, `Queued`, enabled, no
    /// priority, position 0 and no metadata. The remaining Swift
    /// initializer arguments are the `with_*` builders.
    ///
    /// # Panics
    ///
    /// Panics if `components` is empty ("a DownloadItem needs at least one
    /// component") or `output_filename` is empty ("outputFilename must not
    /// be empty"). Deserialization does not run these checks, as in Swift:
    /// `PersistedState.isValid` rejects a corrupt snapshot after decode.
    pub fn new(components: Vec<FileComponent>, output_filename: impl Into<String>) -> Self {
        let output_filename = output_filename.into();
        assert!(
            !components.is_empty(),
            "a DownloadItem needs at least one component"
        );
        assert!(
            !output_filename.is_empty(),
            "outputFilename must not be empty"
        );
        let source_url = components[0].url.clone();
        Self {
            id: Uuid::new_v4(),
            components,
            output_filename,
            source_url,
            assembly: Assembly::None,
            state: ItemState::Queued,
            is_enabled: true,
            priority: None,
            position: 0,
            metadata: None,
        }
    }

    /// The pre-Part-5 signature's core: wraps a single URL into a
    /// one-component HTTP item.
    ///
    /// # Panics
    ///
    /// Panics if `filename` is empty.
    pub fn http(url: SourceUrl, filename: impl Into<String>) -> Self {
        let filename = filename.into();
        let component = FileComponent::new(url, filename.clone());
        Self::new(vec![component], filename)
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_source_url(mut self, source_url: SourceUrl) -> Self {
        self.source_url = source_url;
        self
    }

    pub fn with_assembly(mut self, assembly: Assembly) -> Self {
        self.assembly = assembly;
        self
    }

    pub fn with_state(mut self, state: ItemState) -> Self {
        self.state = state;
        self
    }

    pub fn with_enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = is_enabled;
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn with_position(mut self, position: i64) -> Self {
        self.position = position;
        self
    }

    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Sets the size of a one-component item (see [`Self::set_total_bytes`]).
    ///
    /// # Panics
    ///
    /// Panics unless the item has exactly one component.
    pub fn with_total_bytes(mut self, total_bytes: i64) -> Self {
        self.set_total_bytes(Some(total_bytes));
        self
    }

    // MARK: Concatenated accessors (one-component: identical to before)

    /// The grabbed origin URL (`source_url`). For HTTP this is the file URL;
    /// for YouTube it is the watch URL, not the `googlevideo` stream.
    pub fn url(&self) -> &SourceUrl {
        &self.source_url
    }

    pub fn filename(&self) -> &str {
        &self.output_filename
    }

    pub fn validator(&self) -> Option<&str> {
        self.components[0].validator.as_deref()
    }

    /// Sets the validator on every component.
    pub fn set_validator(&mut self, validator: Option<String>) {
        for component in &mut self.components {
            component.validator.clone_from(&validator);
        }
    }

    /// True when any component is downloaded wholesale (yt-dlp as the
    /// downloader). Such an item can leave yt-dlp scratch files in its
    /// package folder, which remove / reset must sweep. Resumability is no
    /// longer decided here — a wholesale component drives its own
    /// `is_resumable` once it confirms yt-dlp's native downloader is in use.
    /// Parent specs `2026-09-03-multi-site-resolver-design.md` §6.7 and
    /// `2026-09-03-wholesale-resume-design.md`.
    pub fn has_wholesale_component(&self) -> bool {
        self.components
            .iter()
            .any(|c| matches!(c.origin, ComponentOrigin::Wholesale { .. }))
    }

    /// `Some(false)` if any component is non-resumable, else `None` if any is
    /// unknown, else `Some(true)`.
    pub fn is_resumable(&self) -> Option<bool> {
        if self
            .components
            .iter()
            .any(|c| c.is_resumable == Some(false))
        {
            return Some(false);
        }
        if self.components.iter().any(|c| c.is_resumable.is_none()) {
            return None;
        }
        Some(true)
    }

    /// Sets `is_resumable` on every component.
    pub fn set_is_resumable(&mut self, value: Option<bool>) {
        for component in &mut self.components {
            component.is_resumable = value;
        }
    }

    /// The sum of the component sizes, or `None` while any is unknown.
    pub fn total_bytes(&self) -> Option<i64> {
        let mut sum = 0;
        for component in &self.components {
            sum += component.total_bytes?;
        }
        Some(sum)
    }

    /// # Panics
    ///
    /// Panics unless the item has exactly one component ("set totalBytes
    /// only on a one-component item").
    pub fn set_total_bytes(&mut self, value: Option<i64>) {
        assert!(
            self.components.len() == 1,
            "set totalBytes only on a one-component item"
        );
        self.components[0].total_bytes = value;
    }

    /// Item-space completed ranges: each component's ranges shifted by the
    /// sum of the sizes before it. Falls back to component 0 alone while any
    /// size is still unknown.
    pub fn completed(&self) -> RangeSet {
        let bases = self.component_base_offsets();
        if bases.len() != self.components.len() {
            return self.components[0].completed.clone();
        }
        let mut union = RangeSet::new();
        for (index, component) in self.components.iter().enumerate() {
            for range in component.completed.ranges() {
                union.insert(ByteRange::new(
                    range.start + bases[index],
                    range.end + bases[index],
                ));
            }
        }
        union
    }

    /// # Panics
    ///
    /// Panics unless the item has exactly one component ("set completed
    /// only on a one-component item").
    pub fn set_completed(&mut self, value: RangeSet) {
        assert!(
            self.components.len() == 1,
            "set completed only on a one-component item"
        );
        self.components[0].completed = value;
    }

    /// Running prefix sums of component sizes, or empty when any size is
    /// unknown (so callers fall back rather than misplace ranges).
    pub fn component_base_offsets(&self) -> Vec<i64> {
        if self.components.iter().any(|c| c.total_bytes.is_none()) {
            return Vec::new();
        }
        let mut bases = Vec::with_capacity(self.components.len());
        let mut running = 0;
        for component in &self.components {
            bases.push(running);
            running += component.total_bytes.unwrap_or(0);
        }
        bases
    }

    pub fn fraction_completed(&self) -> f64 {
        let Some(total) = self.total_bytes() else {
            return 0.0;
        };
        if total <= 0 {
            return 0.0;
        }
        let done: i64 = self
            .components
            .iter()
            .map(|c| c.completed.total_bytes())
            .sum();
        done as f64 / total as f64
    }

    pub fn is_complete(&self) -> bool {
        self.components.iter().all(FileComponent::is_complete)
    }
}

// MARK: Deserialize (accepts the legacy flat shape and the new nested one)

/// Every key either shape can carry; which fields are required depends on
/// whether `components` is present, as in Swift's `init(from:)`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    id: Uuid,
    state: Option<ItemState>,
    is_enabled: Option<bool>,
    priority: Option<Priority>,
    position: Option<i64>,
    metadata: Option<String>,
    // nested shape
    components: Option<Vec<FileComponent>>,
    output_filename: Option<String>,
    #[serde(rename = "sourceURL")]
    source_url: Option<SourceUrl>,
    assembly: Option<Assembly>,
    // legacy flat shape
    url: Option<SourceUrl>,
    filename: Option<String>,
    total_bytes: Option<i64>,
    completed: Option<RangeSet>,
    is_resumable: Option<bool>,
    validator: Option<String>,
}

impl<'de> Deserialize<'de> for DownloadItem {
    /// Never runs the constructor's preconditions (see [`DownloadItem::new`]).
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        let raw = RawItem::deserialize(deserializer)?;
        let (components, output_filename, assembly, source_url) = match raw.components {
            Some(components) => (
                components,
                raw.output_filename
                    .ok_or_else(|| D::Error::missing_field("outputFilename"))?,
                raw.assembly.unwrap_or(Assembly::None),
                // Absent in state.json written before `sourceURL` existed —
                // fall back to the first component's URL, the old behaviour.
                raw.source_url,
            ),
            None => {
                let filename = raw
                    .filename
                    .ok_or_else(|| D::Error::missing_field("filename"))?;
                let legacy_url = raw.url.ok_or_else(|| D::Error::missing_field("url"))?;
                let mut component = FileComponent::new(legacy_url.clone(), filename.clone());
                component.total_bytes = raw.total_bytes;
                component.completed = raw.completed.unwrap_or_default();
                component.validator = raw.validator;
                component.is_resumable = raw.is_resumable;
                (vec![component], filename, Assembly::None, Some(legacy_url))
            }
        };
        // Swift traps on `components[0]` here; an error is the safe twin.
        let source_url = match source_url {
            Some(url) => url,
            None => components
                .first()
                .map(|c| c.url.clone())
                .ok_or_else(|| D::Error::custom("item has no components and no sourceURL"))?,
        };
        Ok(DownloadItem {
            id: raw.id,
            components,
            output_filename,
            source_url,
            assembly,
            state: raw.state.unwrap_or(ItemState::Queued),
            is_enabled: raw.is_enabled.unwrap_or(true),
            priority: raw.priority,
            position: raw.position.unwrap_or(0),
            metadata: raw.metadata,
        })
    }
}
