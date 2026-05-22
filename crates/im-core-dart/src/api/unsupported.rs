use crate::dto::error::DartImError;

pub fn unsupported(capability: String) -> Result<(), DartImError> {
    Err(DartImError::unsupported(capability))
}
