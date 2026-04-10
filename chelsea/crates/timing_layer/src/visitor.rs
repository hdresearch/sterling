/// Tracing uses the visitor pattern to permit access into new span attributes and event fields, so these structs are
/// how we can extract particular fields/attributes
use tracing::field::Visit;

pub struct RetErrVisitor {
    /// Is only true in the specific scenario that a return value was found, AND it was a Result::Err
    pub is_error: bool,
    pub value: Option<String>,
}

impl Default for RetErrVisitor {
    fn default() -> Self {
        Self {
            is_error: false,
            value: None,
        }
    }
}

impl Visit for RetErrVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "return" {
            self.value = Some(format!("{value:?}"));
        } else if field.name() == "error" {
            self.value = Some(format!("{value:?}"));
            self.is_error = true;
        }
    }
}

pub struct OperationIdVisitor {
    pub operation_id: Option<String>,
}

impl Default for OperationIdVisitor {
    fn default() -> Self {
        Self { operation_id: None }
    }
}

impl Visit for OperationIdVisitor {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {
        // operation_id is expected to be a string
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "operation_id" {
            self.operation_id = Some(format!("{value}"))
        }
    }
}
