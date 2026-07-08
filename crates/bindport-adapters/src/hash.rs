use super::*;

pub(crate) fn short_hash(value: &str) -> String {
    let hash = value.chars().take(8).collect::<String>();

    if hash.is_empty() {
        String::from("00000000")
    } else {
        hash
    }
}

pub(crate) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

pub(crate) fn content_hash(contents: &str) -> String {
    sha256_hex(contents.as_bytes())
}

pub fn rendered_content_hash(contents: &str) -> String {
    content_hash(contents)
}

pub(crate) fn content_hash_matches(contents: &str, expected_hash: &str) -> bool {
    content_hash(contents) == expected_hash || legacy_content_hash(contents) == expected_hash
}

pub(crate) fn legacy_content_hash(contents: &str) -> String {
    format!("{:016x}", stable_hash(contents.as_bytes()))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
