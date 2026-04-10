use std::fmt::Display;

use thiserror::Error;

#[derive(Error, Debug)]
#[error("Unknown chelsea database error. reason: {reason}")]
pub struct StoreError {
    reason: String,
}

impl StoreError {
    pub fn from_display<T>(value: T) -> Self
    where
        T: Display,
    {
        StoreError {
            reason: value.to_string(),
        }
    }
}
