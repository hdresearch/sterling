use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};
use utoipa::ToSchema;
/// A simple struct that represents a qualified rbd snapshot name without the need to constantly parse. It is of the format "{image_name}@{snap_name}"
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
        let parts: Vec<_> = s.split("@").collect();
        if parts.len() != 2 {
            return Err(RbdSnapName::parse_error_msg(s));
        }

        let image_name = parts
            .get(0)
            .ok_or(RbdSnapName::parse_error_msg(s))?
            .to_string();
        let snap_name = parts
            .get(1)
            .ok_or(RbdSnapName::parse_error_msg(s))?
            .to_string();

        Ok(Self {
            image_name,
            snap_name,
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
                    .map_err(|_| E::custom(format!("invalid RbdSnapName: {}", v)))
            }
        }

        deserializer.deserialize_str(RbdSnapNameVisitor)
    }
}
