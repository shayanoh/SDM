use std::fmt;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};

/// Download priority. Encodes as its integer raw value (`0...4`), as Swift's
/// `Int`-backed `Codable` enum does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i64)]
pub enum Priority {
    Lowest = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Highest = 4,
}

impl Priority {
    /// Every case, lowest to highest (Swift `CaseIterable.allCases`).
    pub const ALL_CASES: [Priority; 5] = [
        Priority::Lowest,
        Priority::Low,
        Priority::Normal,
        Priority::High,
        Priority::Highest,
    ];

    pub fn raw_value(self) -> i64 {
        self as i64
    }

    /// The case with this raw value, if any.
    pub fn from_raw_value(raw: i64) -> Option<Priority> {
        Self::ALL_CASES.into_iter().find(|p| p.raw_value() == raw)
    }
}

impl Serialize for Priority {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.raw_value())
    }
}

impl<'de> Deserialize<'de> for Priority {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = i64::deserialize(deserializer)?;
        Priority::from_raw_value(raw).ok_or_else(|| {
            de::Error::custom(format!(
                "Cannot initialize Priority from invalid Int value {raw}"
            ))
        })
    }
}

/// Lifecycle state of a download.
///
/// Two independent axes, on purpose (a reversal of this codebase's earlier
/// single-axis design): `state` is the scheduling axis — `Stopped` means
/// "not desired right now," `Queued` means "eligible, waiting for a slot,"
/// and a preempted item returns to `Queued`, never `Stopped`. `is_enabled` on
/// `DownloadItem` is the orthogonal, purely user-managed axis — "never start
/// this no matter what" — and nothing but an explicit user action may ever
/// change it. Pause All / Resume All only ever touch `state`; Disable /
/// Enable only ever touch `is_enabled`.
///
/// Wire shape is Swift's synthesized enum `Codable`: `{"stopped":{}}`,
/// `{"failed":{"reason":"x"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ItemState {
    Stopped,
    Queued,
    Running,
    Completed,
    Failed { reason: String },
}

#[derive(Serialize)]
struct EmptyObject {}

#[derive(Serialize)]
struct FailedBody<'a> {
    reason: &'a str,
}

impl Serialize for ItemState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            ItemState::Stopped => map.serialize_entry("stopped", &EmptyObject {})?,
            ItemState::Queued => map.serialize_entry("queued", &EmptyObject {})?,
            ItemState::Running => map.serialize_entry("running", &EmptyObject {})?,
            ItemState::Completed => map.serialize_entry("completed", &EmptyObject {})?,
            ItemState::Failed { reason } => {
                map.serialize_entry("failed", &FailedBody { reason })?
            }
        }
        map.end()
    }
}

#[derive(Deserialize)]
struct FailedFields {
    reason: String,
}

struct ItemStateVisitor;

impl<'de> Visitor<'de> for ItemStateVisitor {
    type Value = ItemState;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an ItemState object with exactly one case key")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ItemState, A::Error> {
        let key: String = map
            .next_key()?
            .ok_or_else(|| de::Error::custom("ItemState object has no case key"))?;
        let state = match key.as_str() {
            "failed" => {
                let FailedFields { reason } = map.next_value()?;
                ItemState::Failed { reason }
            }
            "stopped" | "queued" | "running" | "completed" => {
                let _: de::IgnoredAny = map.next_value()?;
                match key.as_str() {
                    "stopped" => ItemState::Stopped,
                    "queued" => ItemState::Queued,
                    "running" => ItemState::Running,
                    _ => ItemState::Completed,
                }
            }
            other => {
                return Err(de::Error::custom(format!(
                    "Unknown ItemState case `{other}`"
                )));
            }
        };
        if map.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(de::Error::custom(
                "ItemState object has more than one case key",
            ));
        }
        Ok(state)
    }
}

impl<'de> Deserialize<'de> for ItemState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ItemStateVisitor)
    }
}
