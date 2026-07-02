// SPDX-License-Identifier: MIT

use super::*;

pub fn object_keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("json object")
        .keys()
        .map(String::as_str)
        .collect()
}

pub fn dotenv_value<'a>(contents: &'a str, name: &str) -> Option<&'a str> {
    contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

pub fn toml_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");

    format!("\"{escaped}\"")
}
