use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValidationIssue {
    pub field: String,
    pub message: String,
}

impl ConfigValidationIssue {
    pub(crate) fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

pub(crate) fn validate_no_control_chars(
    field: &str,
    value: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if value.bytes().any(is_control_byte) {
        issues.push(ConfigValidationIssue::new(field, message));
    }
}

pub(crate) fn validate_no_backticks(
    field: &str,
    value: &str,
    message: &str,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if value.contains('`') {
        issues.push(ConfigValidationIssue::new(field, message));
    }
}

pub(crate) fn is_control_byte(byte: u8) -> bool {
    byte < 0x20 || byte == 0x7f
}
