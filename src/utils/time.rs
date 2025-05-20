use serde::{Deserialize, Deserializer, Serializer};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Deserialize an RFC 3339 formatted string into an OffsetDateTime
pub fn deserialize<'de, D>(deserializer: D) -> Result<OffsetDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    OffsetDateTime::parse(&s, &Rfc3339).map_err(serde::de::Error::custom)
}

/// Serialize an OffsetDateTime into an RFC 3339 formatted string
pub fn serialize<S>(datetime: &OffsetDateTime, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = datetime
        .format(&Rfc3339)
        .map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&s)
}
