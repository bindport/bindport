use super::*;

pub(crate) const BUILT_IN_TRAEFIK: &str = include_str!("../templates/bindport-traefik.yml.j2");
pub(crate) const BUILT_IN_CADDY: &str = include_str!("../templates/bindport-caddy.caddy.j2");
pub(crate) const BUILT_IN_ENV_LOCAL: &str = include_str!("../templates/bindport-env-local.env.j2");
pub(crate) const TEMPLATE_FUEL: u64 = 200_000;
pub(crate) const MAX_RENDERED_TEMPLATE_BYTES: usize = 1024 * 1024;
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
#[derive(Debug)]
pub enum TemplateError {
    InvalidName(String),
    OutputTooLarge {
        bytes: usize,
        limit: usize,
    },
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
            Self::OutputTooLarge { bytes, limit } => write!(
                f,
                "rendered template output is {bytes} bytes, exceeding the {limit} byte limit"
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
    environment.set_fuel(Some(TEMPLATE_FUEL));
    environment.add_filter("dotenv", dotenv_escape);

    let rendered = environment.render_str(template, context)?;
    if rendered.len() > MAX_RENDERED_TEMPLATE_BYTES {
        return Err(TemplateError::OutputTooLarge {
            bytes: rendered.len(),
            limit: MAX_RENDERED_TEMPLATE_BYTES,
        });
    }

    Ok(rendered)
}

pub(crate) fn dotenv_escape(value: String) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| String::from("\"\""))
}
pub(crate) fn validate_template_name(name: &str) -> Result<(), TemplateError> {
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

pub(crate) fn read_template(
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

pub(crate) fn wildcard_matches(
    directory: &Path,
    name: &str,
) -> Result<Vec<PathBuf>, TemplateError> {
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

pub(crate) fn list_directory_templates(
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

pub(crate) fn logical_name_from_filename(filename: &str) -> Option<String> {
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

pub(crate) struct BuiltInTemplate {
    pub(crate) name: &'static str,
    pub(crate) contents: &'static str,
}

pub(crate) fn built_in_templates() -> &'static [BuiltInTemplate] {
    &[
        BuiltInTemplate {
            name: "bindport-traefik",
            contents: BUILT_IN_TRAEFIK,
        },
        BuiltInTemplate {
            name: "bindport-caddy",
            contents: BUILT_IN_CADDY,
        },
        BuiltInTemplate {
            name: "bindport-env-local",
            contents: BUILT_IN_ENV_LOCAL,
        },
    ]
}

pub(crate) fn resolve_built_in(name: &str) -> Option<ResolvedTemplate> {
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
