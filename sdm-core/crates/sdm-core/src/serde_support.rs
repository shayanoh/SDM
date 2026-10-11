//! Serde helpers that reproduce how Swift's `JSONEncoder` writes a few
//! foundation types.

/// `UUID` as Swift writes it: the uppercase hyphenated string. Reading is
/// case-insensitive, like Swift's `UUID(uuidString:)`.
pub(crate) mod uuid_upper {
    use serde::{Deserialize, Deserializer, Serializer};
    use uuid::Uuid;

    pub fn serialize<S: Serializer>(id: &Uuid, serializer: S) -> Result<S::Ok, S::Error> {
        let mut buffer = Uuid::encode_buffer();
        serializer.serialize_str(id.hyphenated().encode_upper(&mut buffer))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uuid, D::Error> {
        Uuid::deserialize(deserializer)
    }
}

/// `Double?` as Swift writes it: a whole number has no fractional part
/// (`3000`, not `3000.0`). Absent is omitted by the field's
/// `skip_serializing_if`.
pub(crate) mod swift_double_opt {
    use serde::{Deserialize, Deserializer, Serializer};

    /// Largest magnitude at which every integer is exactly representable.
    const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

    pub fn serialize<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            None => serializer.serialize_none(),
            Some(v) if v.is_finite() && v.fract() == 0.0 && v.abs() < MAX_EXACT => {
                serializer.serialize_i64(*v as i64)
            }
            Some(v) => serializer.serialize_f64(*v),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        Option::<f64>::deserialize(deserializer)
    }
}
