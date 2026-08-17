use super::*;

pub const DNS_LABEL_MAX_BYTES: usize = 63;
pub const DNS_HOSTNAME_MAX_BYTES: usize = 253;
const TRUNCATE_WITH_HASH_SUFFIX_LENGTH: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostnameLabelChange {
    pub original: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedHostname {
    pub value: String,
    pub changes: Vec<HostnameLabelChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncateWithHashError;

impl fmt::Display for TruncateWithHashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("truncate_with_hash max_length must be at least 10")
    }
}

impl std::error::Error for TruncateWithHashError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsHostnameError {
    message: String,
}

impl DnsHostnameError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DnsHostnameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DnsHostnameError {}

pub fn truncate_with_hash(value: &str, max_length: usize) -> Result<String, TruncateWithHashError> {
    if max_length <= TRUNCATE_WITH_HASH_SUFFIX_LENGTH {
        return Err(TruncateWithHashError);
    }

    if value.chars().count() <= max_length {
        return Ok(value.to_string());
    }

    let prefix_length = max_length - TRUNCATE_WITH_HASH_SUFFIX_LENGTH;
    let prefix = value.chars().take(prefix_length).collect::<String>();
    let hash = sha256_hex(value.as_bytes());

    Ok(format!("{prefix}-{}", &hash[..8]))
}

pub fn normalize_dns_hostname(hostname: &str) -> Result<NormalizedHostname, DnsHostnameError> {
    if hostname.is_empty() {
        return Err(DnsHostnameError::new("hostname must not be empty"));
    }

    let (hostname, root_dot) = match hostname.strip_suffix('.') {
        Some(hostname) => (hostname, true),
        None => (hostname, false),
    };
    if hostname.is_empty() {
        return Err(DnsHostnameError::new("hostname must not be empty"));
    }

    let mut normalized_labels = Vec::new();
    let mut changes = Vec::new();
    for label in hostname.split('.') {
        validate_dns_label(label)?;
        if label.len() > DNS_LABEL_MAX_BYTES {
            let replacement = truncate_with_hash(label, DNS_LABEL_MAX_BYTES)
                .expect("DNS label limit supports the hash suffix");
            changes.push(HostnameLabelChange {
                original: label.to_string(),
                replacement: replacement.clone(),
            });
            normalized_labels.push(replacement);
        } else {
            normalized_labels.push(label.to_string());
        }
    }

    let mut value = normalized_labels.join(".");
    if value.len() > DNS_HOSTNAME_MAX_BYTES {
        return Err(DnsHostnameError::new(format!(
            "hostname is {} bytes and exceeds {} bytes",
            value.len(),
            DNS_HOSTNAME_MAX_BYTES
        )));
    }
    if root_dot {
        value.push('.');
    }

    Ok(NormalizedHostname { value, changes })
}

fn validate_dns_label(label: &str) -> Result<(), DnsHostnameError> {
    if label.is_empty() {
        return Err(DnsHostnameError::new(
            "hostname must not contain an empty label",
        ));
    }
    if !label.is_ascii() {
        return Err(DnsHostnameError::new(format!(
            "hostname label `{label}` must contain only ASCII characters"
        )));
    }
    if label.starts_with('-') || label.ends_with('-') {
        return Err(DnsHostnameError::new(format!(
            "hostname label `{label}` must not start or end with `-`"
        )));
    }
    if let Some(character) = label
        .chars()
        .find(|character| !character.is_ascii_alphanumeric() && *character != '-')
    {
        return Err(DnsHostnameError::new(format!(
            "hostname label `{label}` contains invalid character `{character}`"
        )));
    }

    Ok(())
}
