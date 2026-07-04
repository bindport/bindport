// SPDX-License-Identifier: MIT

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const DESCRIPTION_LIMIT: usize = 170;
const SKIP_DESCRIPTIONS: &[&str] = &["print.html", "toc.html"];
const SITEMAP_SKIP: &[&str] = &["404.html", "print.html", "toc.html"];

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.as_slice() {
        [command] if command == "platform-guard" => platform_guard().map_err(|error| error.to_string()),
        [command, out_dir] if command == "docs-postprocess" => {
            docs_postprocess(Path::new(out_dir)).map_err(|error| error.to_string())
        }
        [command, out_dir, base_url] if command == "docs-sitemap" => {
            docs_sitemap(Path::new(out_dir), base_url).map_err(|error| error.to_string())
        }
        _ => Err(
            "usage: xtask <platform-guard|docs-postprocess <out-dir>|docs-sitemap <out-dir> <base-url>>"
                .to_owned(),
        ),
    }
}

fn docs_postprocess(out_dir: &Path) -> io::Result<()> {
    for path in html_files(out_dir)? {
        update_description(&path)?;
    }
    Ok(())
}

fn docs_sitemap(out_dir: &Path, base_url: &str) -> io::Result<()> {
    let base_url = format!("{}/", base_url.trim_end_matches('/'));
    let mut locations = Vec::new();

    for page in html_files(out_dir)? {
        let rel = page
            .strip_prefix(out_dir)
            .unwrap_or(&page)
            .to_string_lossy()
            .replace('\\', "/");
        if SITEMAP_SKIP.contains(&rel.as_str()) {
            continue;
        }

        let location = if rel == "index.html" {
            base_url.clone()
        } else {
            format!("{base_url}{rel}")
        };
        locations.push(location);
    }

    locations.sort();

    let mut sitemap = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for location in locations {
        sitemap.push_str("  <url><loc>");
        sitemap.push_str(&escape_xml(&location));
        sitemap.push_str("</loc></url>\n");
    }
    sitemap.push_str("</urlset>\n");

    fs::write(out_dir.join("sitemap.xml"), sitemap)?;
    fs::write(
        out_dir.join("robots.txt"),
        format!("User-agent: *\nAllow: /\nSitemap: {base_url}sitemap.xml\n"),
    )?;

    Ok(())
}

fn html_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_html_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_html_files(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if file_type.is_dir() {
            collect_html_files(&path, files)?;
        } else if path.extension() == Some(OsStr::new("html")) {
            files.push(path);
        }
    }
    Ok(())
}

fn update_description(path: &Path) -> io::Result<()> {
    if path
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| SKIP_DESCRIPTIONS.contains(&name))
    {
        return Ok(());
    }

    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let Some(description) = page_description(&contents) else {
        return Ok(());
    };

    let meta = format!(
        "<meta name=\"description\" content=\"{}\">",
        escape_attr(&description)
    );
    let updated = if let Some((start, end)) = find_meta_description(&contents) {
        format!("{}{}{}", &contents[..start], meta, &contents[end..])
    } else if let Some(title_end) = contents.find("</title>") {
        let insert_at = title_end + "</title>".len();
        format!(
            "{}\n        {}{}",
            &contents[..insert_at],
            meta,
            &contents[insert_at..]
        )
    } else {
        return Ok(());
    };

    if updated != contents {
        fs::write(path, updated)?;
    }

    Ok(())
}

fn find_meta_description(contents: &str) -> Option<(usize, usize)> {
    let needle = "<meta name=\"description\" content=\"";
    let start = contents.find(needle)?;
    let rest = &contents[start..];
    let end = rest.find("\">")? + start + 2;
    Some((start, end))
}

fn page_description(contents: &str) -> Option<String> {
    let main = html_element(contents, "main")?;
    let main_without_title = remove_first_element(main, "h1");
    let paragraph = html_element(&main_without_title, "p")?;
    let description = text_from_html(paragraph);
    if description.is_empty() {
        None
    } else {
        Some(trim_description(&description, DESCRIPTION_LIMIT))
    }
}

fn html_element<'a>(contents: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let start_tag = contents.find(&open)?;
    let after_start = contents[start_tag..].find('>')? + start_tag + 1;
    let close = format!("</{tag}>");
    let end = contents[after_start..].find(&close)? + after_start;
    Some(&contents[after_start..end])
}

fn remove_first_element(contents: &str, tag: &str) -> String {
    let Some(start) = contents.find(&format!("<{tag}")) else {
        return contents.to_owned();
    };
    let close = format!("</{tag}>");
    let Some(relative_end) = contents[start..].find(&close) else {
        return contents.to_owned();
    };
    let end = start + relative_end + close.len();
    format!("{}{}", &contents[..start], &contents[end..])
}

fn text_from_html(fragment: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;

    for ch in fragment.chars() {
        match ch {
            '<' => {
                in_tag = true;
                output.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }

    collapse_whitespace(&unescape_html(&output))
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_description(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }

    let clipped = value
        .char_indices()
        .take_while(|(index, _)| *index <= limit)
        .map(|(_, ch)| ch)
        .collect::<String>();
    let mut words = clipped
        .rsplit_once(' ')
        .map_or(clipped.as_str(), |(head, _)| head);
    words = words.trim_end_matches(['.', ',', ';', ':']);
    format!("{words}.")
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml(value: &str) -> String {
    escape_attr(value).replace('\'', "&apos;")
}

fn unescape_html(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn platform_guard() -> Result<(), PlatformGuardError> {
    let mut failures = Vec::new();
    for path in rust_files(Path::new("crates"))? {
        let contents = fs::read_to_string(&path)?;
        let lines = contents.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if !is_let_declaration(line) {
                continue;
            }

            let Some(cfg_index) = next_significant_line(&lines, statement_end(&lines, index) + 1)
            else {
                continue;
            };
            if is_linux_cfg(lines[cfg_index]) {
                failures.push(format!("{}:{}", path.display(), index + 1));
            }
        }
    }

    if failures.is_empty() {
        return Ok(());
    }

    Err(PlatformGuardError::Failures(failures))
}

fn rust_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension() == Some(OsStr::new("rs")) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_let_declaration(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("let ") else {
        return false;
    };
    rest.chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn next_significant_line(lines: &[&str], index: usize) -> Option<usize> {
    (index..lines.len()).find(|current| {
        let stripped = lines[*current].trim();
        !stripped.is_empty() && !stripped.starts_with("//")
    })
}

fn statement_end(lines: &[&str], index: usize) -> usize {
    (index..lines.len())
        .find(|current| lines[*current].contains(';'))
        .unwrap_or(index)
}

fn is_linux_cfg(line: &str) -> bool {
    let normalized = line.split_whitespace().collect::<String>();
    normalized == "#[cfg(target_os=\"linux\")]"
}

#[derive(Debug)]
enum PlatformGuardError {
    Io(io::Error),
    Failures(Vec<String>),
}

impl From<io::Error> for PlatformGuardError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Display for PlatformGuardError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Failures(failures) => {
                writeln!(
                    formatter,
                    "Linux-only cfg guard found declarations immediately before \
                     #[cfg(target_os = \"linux\")] blocks."
                )?;
                writeln!(
                    formatter,
                    "Move declarations used only by Linux cfg blocks inside the cfg block. \
                     Linux clippy treats them as used, but macOS clippy reports them as unused."
                )?;
                for failure in failures {
                    writeln!(formatter, "  {failure}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PlatformGuardError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_content_paragraph() {
        let html = r#"
            <main>
              <h1>Title</h1>
              <p>BindPort <strong>keeps</strong> ports &amp; routes stable.</p>
            </main>
        "#;

        assert_eq!(
            page_description(html).as_deref(),
            Some("BindPort keeps ports & routes stable.")
        );
    }

    #[test]
    fn trims_description_on_word_boundary() {
        let value = trim_description("one two three four five", 13);

        assert_eq!(value, "one two three.");
    }

    #[test]
    fn replaces_existing_meta_description() {
        let html = r#"<head><meta name="description" content="old"></head>"#;
        let (start, end) = find_meta_description(html).expect("meta description");

        assert_eq!(
            &html[start..end],
            r#"<meta name="description" content="old">"#
        );
    }

    #[test]
    fn detects_linux_cfg_after_let_declaration() {
        let lines = [
            "let path = PathBuf::new();",
            "",
            "#[cfg(target_os = \"linux\")]",
            "run(path);",
        ];
        let cfg_index = next_significant_line(&lines, statement_end(&lines, 0) + 1);

        assert_eq!(cfg_index, Some(2));
        assert!(is_linux_cfg(lines[2]));
    }

    #[test]
    fn ignores_non_linux_cfg() {
        assert!(!is_linux_cfg("#[cfg(unix)]"));
    }
}
