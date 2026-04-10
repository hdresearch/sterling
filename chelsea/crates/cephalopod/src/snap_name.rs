//! Qualified RBD snapshot name: `image_name@snap_name`.

use std::fmt::Display;
use std::str::FromStr;

use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use utoipa::ToSchema;

/// A qualified RBD snapshot name in the format `image_name@snap_name`.
///
/// The `image_name` may include a namespace prefix (e.g. `"owner_id/my-image"`).
#[derive(Debug, Clone, ToSchema)]
pub struct RbdSnapName {
    pub image_name: String,
    pub snap_name: String,
}

impl RbdSnapName {
    fn parse_error_msg(input: &str) -> String {
        format!("invalid rbd snap format: {input}")
    }
}

impl Display for RbdSnapName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.image_name, self.snap_name)
    }
}

impl FromStr for RbdSnapName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (image_name, snap_name) = s
            .split_once('@')
            .ok_or_else(|| RbdSnapName::parse_error_msg(s))?;

        if image_name.is_empty() || snap_name.is_empty() {
            return Err(RbdSnapName::parse_error_msg(s));
        }

        Ok(Self {
            image_name: image_name.to_string(),
            snap_name: snap_name.to_string(),
        })
    }
}

impl Serialize for RbdSnapName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for RbdSnapName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RbdSnapNameVisitor;

        impl<'de> Visitor<'de> for RbdSnapNameVisitor {
            type Value = RbdSnapName;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string in the format 'image@snap'")
            }

            fn visit_str<E>(self, v: &str) -> Result<RbdSnapName, E>
            where
                E: serde::de::Error,
            {
                v.parse()
                    .map_err(|_| E::custom(format!("invalid RbdSnapName: {v}")))
            }
        }

        deserializer.deserialize_str(RbdSnapNameVisitor)
    }
}
