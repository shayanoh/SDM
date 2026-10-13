use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::download_item::DownloadItem;
use crate::priority::Priority;
use crate::serde_support::uuid_upper;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPackage {
    #[serde(with = "uuid_upper")]
    pub id: Uuid,
    pub name: String,
    pub items: Vec<DownloadItem>,
    pub priority: Priority,
    /// Position within the download list. Lower sorts earlier.
    pub position: i64,
    /// An optional line shown under the package name — e.g. a truncated
    /// playlist's "50 of 320 videos". Parent spec §5.4. `None` for ordinary
    /// packages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl DownloadPackage {
    /// An empty package with a fresh id, `Normal` priority and position 0.
    ///
    /// # Panics
    ///
    /// Panics if `name` is empty ("package name must not be empty").
    /// Deserialization does not run this check, as in Swift.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(!name.is_empty(), "package name must not be empty");
        Self {
            id: Uuid::new_v4(),
            name,
            items: Vec::new(),
            priority: Priority::Normal,
            position: 0,
            note: None,
        }
    }

    pub fn with_id(mut self, id: Uuid) -> Self {
        self.id = id;
        self
    }

    pub fn with_items(mut self, items: Vec<DownloadItem>) -> Self {
        self.items = items;
        self
    }

    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_position(mut self, position: i64) -> Self {
        self.position = position;
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// An item's own priority when set, otherwise the package's.
    pub fn effective_priority(&self, item: &DownloadItem) -> Priority {
        item.priority.unwrap_or(self.priority)
    }
}
