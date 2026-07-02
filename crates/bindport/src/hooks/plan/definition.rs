use super::*;

pub(crate) fn hook_definition(
    name: &str,
    events: &[HookEvent],
    command: &[String],
    timeout_ms: u64,
    _source: &str,
) -> String {
    let mut definition = String::from("schema=v1\n");
    append_fingerprinted_field(&mut definition, "name", name);
    definition.push_str(&format!("timeout_ms={timeout_ms}\n"));
    definition.push_str(&format!("events={}\n", events.len()));
    for event in events {
        append_fingerprinted_field(&mut definition, "event", event.as_str());
    }
    definition.push_str(&format!("command={}\n", command.len()));
    for value in command {
        append_fingerprinted_field(&mut definition, "argv", value);
    }

    definition
}

pub(crate) fn append_fingerprinted_field(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push(':');
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push('\n');
}

pub(crate) fn stable_hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}
