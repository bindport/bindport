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
    let mut previous = '\n';
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
            previous = character;
            continue;
        }
        if in_single_quote {
            if character == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    previous = '\'';
                    continue;
                }
                in_single_quote = false;
            }
            previous = character;
            continue;
        }

        match character {
            '#' => {
                for next in chars.by_ref() {
                    previous = next;
                    if next == '\n' {
                        break;
                    }
                }
                continue;
            }
            '"' => in_double_quote = true,
            '\'' => in_single_quote = true,
            '&' | '*'
                if yaml_token_boundary(previous)
                    && chars.peek().is_some_and(|next| {
                        next.is_ascii_alphanumeric() || matches!(next, '_' | '-')
                    }) =>
            {
                return true;
            }
            _ => {}
        }
        previous = character;
    }

    false
}

pub(crate) fn yaml_token_boundary(character: char) -> bool {
    character.is_whitespace() || matches!(character, ':' | '-' | ',' | '[' | '{')
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
