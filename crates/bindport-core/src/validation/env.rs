use super::*;

pub(crate) fn validate_service_env(
    index: usize,
    env: Option<&BTreeMap<String, String>>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(env) = env else {
        return;
    };

    for name in env.keys() {
        let field = format!("services[{index}].env.{name}");
        if !is_valid_env_name(name) {
            issues.push(ConfigValidationIssue::new(
                field,
                "service env name must contain only ASCII letters, digits, or `_`, and must not start with a digit",
            ));
        } else if is_restricted_service_env_name(name) {
            issues.push(ConfigValidationIssue::new(
                field,
                "service env name can affect child process execution and must be passed explicitly on the CLI",
            ));
        }
    }
}

pub(crate) fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub fn is_restricted_service_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();

    upper == "PATH"
        || upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("MALLOC_")
        || upper == "NODE_OPTIONS"
        || upper == "BASH_ENV"
        || upper == "ENV"
        || upper == "GCONV_PATH"
        || upper == "LOCPATH"
        || upper == "NLSPATH"
        || upper == "PYTHONPATH"
        || upper == "PYTHONHOME"
        || upper == "PERL5OPT"
        || upper == "PERL5LIB"
        || upper == "RUBYOPT"
        || upper == "GEM_HOME"
        || upper == "GEM_PATH"
        || upper == "IFS"
        || upper == "SHELLOPTS"
        || upper == "CDPATH"
        || upper.starts_with("GIT_CONFIG_")
}
