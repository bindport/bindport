// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use minijinja::{AutoEscape, Environment, UndefinedBehavior};

const BUILT_IN_TRAEFIK: &str = include_str!("../templates/bindport-traefik.yml.j2");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    Traefik,
}

impl AdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Traefik => "traefik",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TemplateSource {
    Project,
    Global,
    BuiltIn,
}

impl TemplateSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
            Self::BuiltIn => "built-in",
        }
    }
}

impl fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSummary {
    pub name: String,
    pub source: TemplateSource,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTemplate {
    pub name: String,
    pub source: TemplateSource,
    pub path: Option<PathBuf>,
    pub contents: String,
    pub wildcard_matches: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TemplateResolver {
    project_templates: Option<PathBuf>,
    global_templates: Option<PathBuf>,
}

impl TemplateResolver {
    pub fn new(project_templates: Option<PathBuf>, global_templates: Option<PathBuf>) -> Self {
        Self {
            project_templates,
            global_templates,
        }
    }

    pub fn resolve(
        &self,
        name: &str,
        source: Option<TemplateSource>,
    ) -> Result<ResolvedTemplate, TemplateError> {
        validate_template_name(name)?;

        let sources = match source {
            Some(source) => vec![source],
            None => vec![
                TemplateSource::Project,
                TemplateSource::Global,
                TemplateSource::BuiltIn,
            ],
        };

        for source in sources {
            match self.resolve_from_source(name, source)? {
                Some(template) => return Ok(template),
                None => continue,
            }
        }

        Err(TemplateError::NotFound {
            name: name.to_string(),
            source,
        })
    }

    pub fn list(
        &self,
        source: Option<TemplateSource>,
    ) -> Result<Vec<TemplateSummary>, TemplateError> {
        let mut templates = BTreeMap::<String, TemplateSummary>::new();
        let sources = match source {
            Some(source) => vec![source],
            None => vec![
                TemplateSource::Project,
                TemplateSource::Global,
                TemplateSource::BuiltIn,
            ],
        };

        for source in sources {
            for summary in self.list_source(source)? {
                templates.entry(summary.name.clone()).or_insert(summary);
            }
        }

        Ok(templates.into_values().collect())
    }

    fn resolve_from_source(
        &self,
        name: &str,
        source: TemplateSource,
    ) -> Result<Option<ResolvedTemplate>, TemplateError> {
        match source {
            TemplateSource::Project => {
                self.resolve_from_directory(name, source, self.project_templates.as_deref())
            }
            TemplateSource::Global => {
                self.resolve_from_directory(name, source, self.global_templates.as_deref())
            }
            TemplateSource::BuiltIn => Ok(resolve_built_in(name)),
        }
    }

    fn resolve_from_directory(
        &self,
        name: &str,
        source: TemplateSource,
        directory: Option<&Path>,
    ) -> Result<Option<ResolvedTemplate>, TemplateError> {
        let Some(directory) = directory else {
            return Ok(None);
        };

        let exact = directory.join(name);
        if exact.is_file() {
            return read_template(name, source, exact, Vec::new()).map(Some);
        }

        let j2 = directory.join(format!("{name}.j2"));
        if j2.is_file() {
            return read_template(name, source, j2, Vec::new()).map(Some);
        }

        let matches = wildcard_matches(directory, name)?;
        let Some(path) = matches.first().cloned() else {
            return Ok(None);
        };

        read_template(name, source, path, matches).map(Some)
    }

    fn list_source(&self, source: TemplateSource) -> Result<Vec<TemplateSummary>, TemplateError> {
        match source {
            TemplateSource::Project => {
                list_directory_templates(source, self.project_templates.as_deref())
            }
            TemplateSource::Global => {
                list_directory_templates(source, self.global_templates.as_deref())
            }
            TemplateSource::BuiltIn => Ok(built_in_templates()
                .iter()
                .map(|template| TemplateSummary {
                    name: template.name.to_string(),
                    source,
                    path: None,
                })
                .collect()),
        }
    }
}

#[derive(Debug)]
pub enum TemplateError {
    InvalidName(String),
    Io {
        path: PathBuf,
        source: io::Error,
    },
    NotFound {
        name: String,
        source: Option<TemplateSource>,
    },
    Render(minijinja::Error),
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(name) => write!(
                f,
                "invalid template name `{name}`; use a safe relative name with no path separators or `..`"
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NotFound { name, source } => match source {
                Some(source) => write!(f, "template `{name}` not found in {source} templates"),
                None => write!(f, "template `{name}` not found"),
            },
            Self::Render(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TemplateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Render(error) => Some(error),
            _ => None,
        }
    }
}

impl From<minijinja::Error> for TemplateError {
    fn from(error: minijinja::Error) -> Self {
        Self::Render(error)
    }
}

pub fn render_template<S: serde::Serialize>(
    template: &str,
    context: S,
) -> Result<String, TemplateError> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment.set_auto_escape_callback(|_| AutoEscape::None);

    Ok(environment.render_str(template, context)?)
}

fn validate_template_name(name: &str) -> Result<(), TemplateError> {
    let invalid = name.is_empty()
        || name.trim() != name
        || name == "."
        || name == ".."
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || Path::new(name).is_absolute();

    if invalid {
        Err(TemplateError::InvalidName(name.to_string()))
    } else {
        Ok(())
    }
}

fn read_template(
    name: &str,
    source: TemplateSource,
    path: PathBuf,
    wildcard_matches: Vec<PathBuf>,
) -> Result<ResolvedTemplate, TemplateError> {
    let contents = fs::read_to_string(&path).map_err(|error| TemplateError::Io {
        path: path.clone(),
        source: error,
    })?;

    Ok(ResolvedTemplate {
        name: name.to_string(),
        source,
        path: Some(path),
        contents,
        wildcard_matches,
    })
}

fn wildcard_matches(directory: &Path, name: &str) -> Result<Vec<PathBuf>, TemplateError> {
    let prefix = format!("{name}.");
    let suffix = ".j2";
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(TemplateError::Io {
                path: directory.to_path_buf(),
                source: error,
            });
        }
    };
    let mut matches = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| TemplateError::Io {
            path: directory.to_path_buf(),
            source: error,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if filename.starts_with(&prefix)
            && filename.ends_with(suffix)
            && filename.len() > prefix.len() + suffix.len()
        {
            matches.push(path);
        }
    }

    matches.sort();
    Ok(matches)
}

fn list_directory_templates(
    source: TemplateSource,
    directory: Option<&Path>,
) -> Result<Vec<TemplateSummary>, TemplateError> {
    let Some(directory) = directory else {
        return Ok(Vec::new());
    };
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(TemplateError::Io {
                path: directory.to_path_buf(),
                source: error,
            });
        }
    };
    let mut templates = BTreeMap::<String, TemplateSummary>::new();

    for entry in entries {
        let entry = entry.map_err(|error| TemplateError::Io {
            path: directory.to_path_buf(),
            source: error,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(name) = logical_name_from_filename(filename) else {
            continue;
        };

        templates.entry(name.clone()).or_insert(TemplateSummary {
            name,
            source,
            path: Some(path),
        });
    }

    Ok(templates.into_values().collect())
}

fn logical_name_from_filename(filename: &str) -> Option<String> {
    if filename.is_empty() || filename.starts_with('.') {
        return None;
    }

    let name = if let Some(prefix) = filename.strip_suffix(".j2") {
        prefix.split('.').next().unwrap_or(prefix)
    } else {
        filename
    };

    validate_template_name(name).ok()?;
    Some(name.to_string())
}

struct BuiltInTemplate {
    name: &'static str,
    contents: &'static str,
}

fn built_in_templates() -> &'static [BuiltInTemplate] {
    &[BuiltInTemplate {
        name: "bindport-traefik",
        contents: BUILT_IN_TRAEFIK,
    }]
}

fn resolve_built_in(name: &str) -> Option<ResolvedTemplate> {
    built_in_templates()
        .iter()
        .find(|template| template.name == name)
        .map(|template| ResolvedTemplate {
            name: template.name.to_string(),
            source: TemplateSource::BuiltIn,
            path: None,
            contents: template.contents.to_string(),
            wildcard_matches: Vec::new(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traefik_is_first_adapter_name() {
        assert_eq!(AdapterKind::Traefik.as_str(), "traefik");
    }

    #[test]
    fn rejects_unsafe_template_names() {
        for name in [
            "",
            " ../x",
            "../x",
            "nested/name",
            "nested\\name",
            "safe..ish",
        ] {
            assert!(matches!(
                validate_template_name(name),
                Err(TemplateError::InvalidName(_))
            ));
        }

        validate_template_name("bindport-traefik").expect("safe template name");
    }

    #[test]
    fn resolves_project_before_global_before_builtin() {
        let root = temp_test_dir("resolver-priority");
        let project = root.join("project");
        let global = root.join("global");
        fs::create_dir_all(&project).expect("project dir");
        fs::create_dir_all(&global).expect("global dir");
        fs::write(project.join("bindport-traefik"), "project template").expect("project template");
        fs::write(global.join("bindport-traefik"), "global template").expect("global template");

        let resolver = TemplateResolver::new(Some(project), Some(global));
        let resolved = resolver
            .resolve("bindport-traefik", None)
            .expect("project template wins");

        assert_eq!(resolved.source, TemplateSource::Project);
        assert_eq!(resolved.contents, "project template");
    }

    #[test]
    fn resolves_global_before_builtin() {
        let root = temp_test_dir("resolver-global");
        let global = root.join("global");
        fs::create_dir_all(&global).expect("global dir");
        fs::write(global.join("bindport-traefik.j2"), "global template").expect("global template");

        let resolver = TemplateResolver::new(None, Some(global));
        let resolved = resolver
            .resolve("bindport-traefik", None)
            .expect("global template wins");

        assert_eq!(resolved.source, TemplateSource::Global);
        assert_eq!(resolved.contents, "global template");
    }

    #[test]
    fn resolves_wildcard_templates_lexicographically() {
        let root = temp_test_dir("resolver-wildcard");
        fs::create_dir_all(&root).expect("template dir");
        fs::write(root.join("app.yaml.j2"), "yaml").expect("yaml template");
        fs::write(root.join("app.00.yml.j2"), "first").expect("first template");
        fs::write(root.join("app.toml.j2"), "toml").expect("toml template");

        let resolver = TemplateResolver::new(Some(root), None);
        let resolved = resolver
            .resolve("app", None)
            .expect("wildcard template resolves");

        assert_eq!(resolved.contents, "first");
        assert_eq!(resolved.wildcard_matches.len(), 3);
    }

    #[test]
    fn render_template_is_strict_and_unescaped() {
        let rendered = render_template(
            "value={{ value }}",
            minijinja::context! {
                value => "<not escaped>",
            },
        )
        .expect("template renders");

        assert_eq!(rendered, "value=<not escaped>");
        assert!(render_template("{{ missing }}", minijinja::context! {}).is_err());
    }

    #[test]
    fn built_in_traefik_template_renders_active_route() {
        let template = TemplateResolver::new(None, None)
            .resolve("bindport-traefik", None)
            .expect("built-in template");
        let rendered = render_template(
            &template.contents,
            minijinja::context! {
                route => minijinja::context! {
                    key => "demo:web:feature",
                    state => "active",
                    hostname => "feature.demo.localhost",
                    slug => "demo-web-feature",
                    target_url => "http://127.0.0.1:29100",
                },
                vars => minijinja::context! {},
            },
        )
        .expect("built-in template renders");

        assert!(rendered.contains("Host(`feature.demo.localhost`)"));
        assert!(rendered.contains("url: \"http://127.0.0.1:29100\""));
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("bindport-{name}-{unique}"));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }
}
