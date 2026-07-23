// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn default_range_matches_roadmap() {
    assert_eq!(DEFAULT_PORT_RANGE.start, 29_000);
    assert_eq!(DEFAULT_PORT_RANGE.end, 29_999);
    assert_eq!(DEFAULT_PORT_RANGE.len(), 1_000);
}

#[test]
fn inverted_range_is_empty() {
    let range = PortRange { start: 100, end: 0 };

    assert!(range.is_empty());
    assert_eq!(range.len(), 0);
}

#[test]
fn default_skiplist_marks_reserved_ports() {
    assert!(is_default_skip_port(29_000));
    assert!(is_default_skip_port(29_999));
    assert!(!is_default_skip_port(29_500));
}

#[test]
fn parses_port_range() {
    assert_eq!(
        parse_port_range("29100-29199").expect("range"),
        PortRange {
            start: 29_100,
            end: 29_199
        }
    );
    assert_eq!(
        parse_port_range(" 00001 - 00002 ").expect("whitespace and leading zeroes"),
        PortRange { start: 1, end: 2 }
    );
    assert_eq!(
        parse_port_range("29100")
            .expect_err("missing separator")
            .to_string(),
        "expected START-END"
    );
    assert_eq!(
        parse_port_range("start-29199")
            .expect_err("invalid start")
            .to_string(),
        "invalid range start `start`"
    );
    assert_eq!(
        parse_port_range("0-29199")
            .expect_err("zero start")
            .to_string(),
        "range start 0 must be at least 1"
    );
    assert_eq!(
        parse_port_range("29100-end")
            .expect_err("invalid end")
            .to_string(),
        "invalid range end `end`"
    );
    assert!(matches!(
        parse_port_range("29199-29100"),
        Err(PortRangeParseError::Empty(_))
    ));
}
