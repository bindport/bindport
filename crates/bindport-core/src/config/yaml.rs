use super::*;

pub(crate) fn validate_yaml_config_source(contents: &str) -> Result<(), String> {
    if contents.len() > MAX_YAML_CONFIG_BYTES {
        return Err(format!(
            "YAML config exceeds {} byte limit",
            MAX_YAML_CONFIG_BYTES
        ));
    }
    if yaml_contains_anchor_or_alias(contents) {
        return Err(String::from(
            "YAML anchors and aliases are not supported in BindPort config",
        ));
    }

    Ok(())
}

pub(crate) fn yaml_contains_anchor_or_alias(contents: &str) -> bool {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    let mut at_node_start = true;
    let mut flow_depth = 0_u32;
    let mut chars = contents.chars().peekable();

    while let Some(character) = chars.next() {
        if in_double_quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_double_quote = false;
            }
            continue;
        }
        if in_single_quote {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    continue;
                }
                in_single_quote = false;
            }
            continue;
        }

        match character {
            '\n' => at_node_start = true,
            character if character.is_whitespace() => {}
            '#' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        at_node_start = true;
                        break;
                    }
                }
            }
            '"' => {
                in_double_quote = true;
                at_node_start = false;
            }
            '\'' => {
                in_single_quote = true;
                at_node_start = false;
            }
            '&' | '*'
                if at_node_start
                    && chars.peek().is_some_and(|next| {
                        next.is_ascii_alphanumeric() || matches!(next, '_' | '-')
                    }) =>
            {
                return true;
            }
            '[' | '{' => {
                flow_depth += 1;
                at_node_start = true;
            }
            ']' | '}' => {
                flow_depth = flow_depth.saturating_sub(1);
                at_node_start = false;
            }
            ',' if flow_depth > 0 => at_node_start = true,
            ':' if flow_depth > 0 || chars.peek().is_none_or(|next| next.is_whitespace()) => {
                at_node_start = true;
            }
            '-' if at_node_start => {
                let mut lookahead = chars.clone();
                if lookahead.next() == Some('-')
                    && lookahead.next() == Some('-')
                    && lookahead.next().is_none_or(|next| next.is_whitespace())
                {
                    chars.next();
                    chars.next();
                } else if !chars.peek().is_some_and(|next| next.is_whitespace()) {
                    at_node_start = false;
                }
            }
            '?' if at_node_start && chars.peek().is_some_and(|next| next.is_whitespace()) => {}
            '!' if at_node_start => {
                while chars.peek().is_some_and(|next| {
                    !next.is_whitespace() && !matches!(next, ',' | '[' | ']' | '{' | '}')
                }) {
                    chars.next();
                }
            }
            _ => at_node_start = false,
        }
    }

    false
}

pub(crate) fn unknown_config_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut keys = keys
        .into_iter()
        .filter(|key| !APPLIED_CONFIG_KEYS.contains(key))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}
