/// It is highly recommended to use newtypes when storing extensions on tracing spans; Extensions.get() uses types,
/// rather than key/value pairs, so other layers may clobber the extensions if you use common types.
use std::{ops::Deref, time::Instant};

#[derive(Clone)]
pub struct TimingStartInstant(Instant);

impl TimingStartInstant {
    pub fn now() -> Self {
        Self(Instant::now())
    }
}

impl Deref for TimingStartInstant {
    type Target = Instant;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Clone)]
pub struct TimingOperationId(pub String);

impl TimingOperationId {
    pub fn new<S: Into<String>>(value: S) -> Self {
        Self(value.into())
    }
}

impl Deref for TimingOperationId {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Into<String> for TimingOperationId {
    fn into(self) -> String {
        self.0
    }
}

impl From<String> for TimingOperationId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
