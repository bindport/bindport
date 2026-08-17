// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn hash_truncation_preserves_short_values_and_is_deterministic() {
    assert_eq!(
        truncate_with_hash("short-name", 12).expect("short value"),
        "short-name"
    );
    assert_eq!(
        truncate_with_hash("exact-length", 12).expect("exact value"),
        "exact-length"
    );

    let value = "feature-this-is-a-very-long-branch-name";
    let first = truncate_with_hash(value, 20).expect("long value");
    let repeated = truncate_with_hash(value, 20).expect("repeated value");
    let different = truncate_with_hash("feature-this-is-a-very-long-branch-name-two", 20)
        .expect("different value");

    assert_eq!(first, "feature-thi-c89948b5");
    assert_eq!(first, repeated);
    assert_ne!(first, different);
    assert_eq!(first.chars().count(), 20);
    assert_eq!(
        truncate_with_hash(&first, 20).expect("already truncated"),
        first
    );
}

#[test]
fn hash_truncation_counts_unicode_characters_and_rejects_short_limits() {
    let value = truncate_with_hash("ééééééééééé", 10).expect("unicode value");

    assert_eq!(value.chars().count(), 10);
    assert!(value.starts_with("é-"));
    assert_eq!(
        truncate_with_hash("value", 9)
            .expect_err("short limit")
            .to_string(),
        "truncate_with_hash max_length must be at least 10"
    );
}

#[test]
fn dns_hostname_normalization_shortens_each_overlong_label() {
    let first = format!("{}-first", "a".repeat(64));
    let second = format!("{}-second", "b".repeat(64));
    let hostname = format!("{first}.{second}.example.localhost");
    let normalized = normalize_dns_hostname(&hostname).expect("hostname");
    let labels = normalized
        .value
        .trim_end_matches('.')
        .split('.')
        .collect::<Vec<_>>();

    assert_eq!(labels.len(), 4);
    assert_eq!(labels[0].len(), DNS_LABEL_MAX_BYTES);
    assert_eq!(labels[1].len(), DNS_LABEL_MAX_BYTES);
    assert_eq!(labels[2], "example");
    assert_eq!(labels[3], "localhost");
    assert_eq!(normalized.changes.len(), 2);
    assert_eq!(normalized.changes[0].original, first);
    assert_eq!(normalized.changes[0].replacement, labels[0]);
    assert_eq!(normalized.changes[1].original, second);
    assert_eq!(normalized.changes[1].replacement, labels[1]);

    let repeated = normalize_dns_hostname(&normalized.value).expect("normalized hostname");
    assert_eq!(repeated.value, normalized.value);
    assert!(repeated.changes.is_empty());
}

#[test]
fn dns_hostname_normalization_preserves_valid_names_and_root_dot() {
    for hostname in [
        "feature.example.localhost",
        "127.0.0.1",
        "UPPER.example",
        "a-b.example.",
    ] {
        let normalized = normalize_dns_hostname(hostname).expect("valid hostname");
        assert_eq!(normalized.value, hostname);
        assert!(normalized.changes.is_empty());
    }

    let longest = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(longest.len(), DNS_HOSTNAME_MAX_BYTES);
    assert_eq!(
        normalize_dns_hostname(&longest)
            .expect("maximum hostname")
            .value,
        longest
    );
}

#[test]
fn dns_hostname_normalization_keeps_long_shared_prefixes_distinct() {
    let prefix = "feature-this-is-a-shared-prefix-that-keeps-going-for-a-long-time";
    let first = normalize_dns_hostname(&format!("{prefix}-one.localhost")).expect("first");
    let second = normalize_dns_hostname(&format!("{prefix}-two.localhost")).expect("second");

    assert_ne!(first.value, second.value);
    assert_eq!(first.value.split('.').next().unwrap().len(), 63);
    assert_eq!(second.value.split('.').next().unwrap().len(), 63);
}

#[test]
fn dns_hostname_normalization_rejects_invalid_names() {
    let overlong_hostname = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(62)
    );

    for (hostname, message) in [
        (String::new(), "must not be empty"),
        (String::from(".example"), "empty label"),
        (String::from("example..localhost"), "empty label"),
        (
            String::from("-example.localhost"),
            "must not start or end with `-`",
        ),
        (
            String::from("example-.localhost"),
            "must not start or end with `-`",
        ),
        (String::from("bad_name.localhost"), "invalid character `_`"),
        (String::from("café.localhost"), "must contain only ASCII"),
        (overlong_hostname, "exceeds 253 bytes"),
    ] {
        let error = normalize_dns_hostname(&hostname).expect_err("invalid hostname");
        assert!(
            error.to_string().contains(message),
            "hostname `{hostname}` produced `{error}`"
        );
    }
}
