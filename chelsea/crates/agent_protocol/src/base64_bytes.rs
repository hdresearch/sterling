//! Serde helper for serializing `Vec<u8>` as a base64 string on the wire.
//!
//! Fields annotated with `#[serde(with = "base64_bytes")]` will be
//! transparently encoded/decoded as base64 in JSON while remaining
//! `Vec<u8>` in Rust code.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{self, Deserialize, Deserializer, Serializer};

pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = STANDARD.encode(bytes);
    serializer.serialize_str(&encoded)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    STANDARD
        .decode(s.as_bytes())
        .map_err(serde::de::Error::custom)
}
