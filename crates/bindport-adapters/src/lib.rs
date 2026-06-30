// SPDX-License-Identifier: MIT

use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use bindport_core::{EffectiveOutputConfig, normalize_branch_label};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRenderConfig {
    pub context: OutputContext,
    pub target_host: String,
    pub target_scheme: String,
    pub vars: BTreeMap<String, serde_json::Value>,
}

impl From<&EffectiveOutputConfig> for OutputRenderConfig {
    fn from(config: &EffectiveOutputConfig) -> Self {
        Self {
            context: OutputContext {
                name: config.name.clone(),
                template: config.template.clone(),
                root: config.root.clone(),
                target: config.target.clone(),
                auto_render: config.auto_render,
                delete_on: config
                    .delete_on
                    .iter()
                    .map(|state| state.as_str().to_string())
                    .collect(),
                on_failure: config.on_failure.as_str().to_string(),
            },
            target_host: config.target_host.clone(),
            target_scheme: config.target_scheme.clone(),
            vars: config.vars.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutputContext {
    pub name: String,
    pub template: String,
    pub root: Option<String>,
    pub target: String,
    pub auto_render: bool,
    pub delete_on: Vec<String>,
    pub on_failure: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRecord {
    pub key: String,
    pub project: String,
    pub service: String,
    pub state: String,
    pub health: String,
    pub port: u16,
    pub host: String,
    pub url: String,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub branch: Option<String>,
    pub branch_label: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_hash: Option<String>,
    pub pid: Option<u32>,
    pub command: String,
    pub cwd: String,
    pub started_at: String,
    pub updated_at: String,
}

impl RouteRecord {
    fn context(&self, output: &OutputRenderConfig) -> RouteContext {
        let worktree_label = self
            .worktree_path
            .as_deref()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .map(normalize_branch_label)
            .unwrap_or_else(|| normalize_branch_label(&self.project));
        let slug = normalize_branch_label(&format!(
            "{}-{}-{}",
            self.project,
            self.service,
            self.branch_label.as_deref().unwrap_or(&worktree_label)
        ));
        let unique_slug = format!(
            "{slug}-{}",
            self.worktree_hash
                .as_deref()
                .map(short_hash)
                .unwrap_or_else(|| format!("{:08x}", stable_hash(self.key.as_bytes()) as u32))
        );
        let target_url = format!(
            "{}://{}:{}",
            output.target_scheme, output.target_host, self.port
        );

        RouteContext {
            key: self.key.clone(),
            project: self.project.clone(),
            service: self.service.clone(),
            state: self.state.clone(),
            health: self.health.clone(),
            port: self.port,
            host: self.host.clone(),
            url: self.url.clone(),
            hostname: self.hostname.clone(),
            route_url: self.route_url.clone(),
            target_url,
            branch: self.branch.clone(),
            branch_label: self.branch_label.clone(),
            worktree_path: self.worktree_path.clone(),
            worktree_label,
            worktree_hash: self.worktree_hash.clone(),
            slug,
            unique_slug,
            pid: self.pid,
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            started_at: self.started_at.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RouteContext {
    pub key: String,
    pub project: String,
    pub service: String,
    pub state: String,
    pub health: String,
    pub port: u16,
    pub host: String,
    pub url: String,
    pub hostname: Option<String>,
    pub route_url: Option<String>,
    pub target_url: String,
    pub branch: Option<String>,
    pub branch_label: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_label: String,
    pub worktree_hash: Option<String>,
    pub slug: String,
    pub unique_slug: String,
    pub pid: Option<u32>,
    pub command: String,
    pub cwd: String,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RenderContext {
    pub route: RouteContext,
    pub output: OutputContext,
    pub vars: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderedRouteFile {
    pub route_key: String,
    pub target: String,
    pub contents: String,
    pub context: RenderContext,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    pub output: OutputContext,
    pub files: Vec<RenderedRouteFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputFileOwnership {
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovableOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub status: OutputFileRemovalStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFileRemovalStatus {
    Removed,
    Missing,
    ExternalModified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenOutputFile {
    pub route_key: String,
    pub path: PathBuf,
    pub content_hash: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOutputFile {
    pub route_key: String,
    pub target: String,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum RenderError {
    TargetTemplate {
        route_key: String,
        source: TemplateError,
    },
    BodyTemplate {
        route_key: String,
        source: TemplateError,
    },
    TargetCollision {
        target: String,
        route_keys: Vec<String>,
    },
}

#[derive(Debug)]
pub enum OutputFileError {
    UnsafeRoot { root: String },
    UnsafeTarget { target: String },
    TargetEscapesRoot { target: String, root: PathBuf },
    SymlinkInPath { path: PathBuf },
    UnownedTarget { path: PathBuf },
    ExternalModified { path: PathBuf },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TargetTemplate { route_key, source } => {
                write!(
                    f,
                    "failed to render target for route `{route_key}`: {source}"
                )
            }
            Self::BodyTemplate { route_key, source } => {
                write!(
                    f,
                    "failed to render template for route `{route_key}`: {source}"
                )
            }
            Self::TargetCollision { target, route_keys } => write!(
                f,
                "multiple routes render to target `{target}`: {}",
                route_keys.join(", ")
            ),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TargetTemplate { source, .. } | Self::BodyTemplate { source, .. } => Some(source),
            Self::TargetCollision { .. } => None,
        }
    }
}

impl fmt::Display for OutputFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeRoot { root } => write!(f, "unsafe output root `{root}`"),
            Self::UnsafeTarget { target } => write!(f, "unsafe output target `{target}`"),
            Self::TargetEscapesRoot { target, root } => write!(
                f,
                "output target `{target}` escapes output root `{}`",
                root.display()
            ),
            Self::SymlinkInPath { path } => {
                write!(f, "output path contains a symlink: {}", path.display())
            }
            Self::UnownedTarget { path } => write!(
                f,
                "refusing to overwrite unowned output file `{}`",
                path.display()
            ),
            Self::ExternalModified { path } => write!(
                f,
                "refusing to overwrite externally modified output file `{}`",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for OutputFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn render_output_routes(
    output: &OutputRenderConfig,
    template: &str,
    routes: &[RouteRecord],
) -> Result<RenderPlan, RenderError> {
    let mut targets = BTreeMap::<String, String>::new();
    let mut files = Vec::with_capacity(routes.len());

    for route in routes {
        let context = RenderContext {
            route: route.context(output),
            output: output.context.clone(),
            vars: output.vars.clone(),
        };
        let target = render_template(&output.context.target, &context).map_err(|source| {
            RenderError::TargetTemplate {
                route_key: route.key.clone(),
                source,
            }
        })?;

        if let Some(existing) = targets.insert(target.clone(), route.key.clone()) {
            return Err(RenderError::TargetCollision {
                target,
                route_keys: vec![existing, route.key.clone()],
            });
        }

        let contents =
            render_template(template, &context).map_err(|source| RenderError::BodyTemplate {
                route_key: route.key.clone(),
                source,
            })?;
        files.push(RenderedRouteFile {
            route_key: route.key.clone(),
            target,
            contents,
            context,
        });
    }

    Ok(RenderPlan {
        output: output.context.clone(),
        files,
    })
}

pub fn write_render_plan(
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[OutputFileOwnership],
) -> Result<Vec<WrittenOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);
    let planned_files = render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)?;
    let owned_hashes = ownership
        .iter()
        .map(|owned| (owned.path.clone(), owned.content_hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut written = Vec::with_capacity(plan.files.len());

    for (file, planned) in plan.files.iter().zip(planned_files) {
        let path = planned.path;
        verify_existing_target(&path, &owned_hashes)?;
        atomic_write(&symlink_anchor, &path, &file.contents)?;

        written.push(WrittenOutputFile {
            route_key: file.route_key.clone(),
            path,
            content_hash: content_hash(&file.contents),
            bytes: file.contents.len(),
        });
    }

    Ok(written)
}

pub fn remove_owned_output_files(
    files: &[RemovableOutputFile],
    base_dir: &Path,
    output: &OutputContext,
) -> Result<Vec<RemovedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, output);
    let mut removed = Vec::with_capacity(files.len());

    for file in files {
        if !file.path.starts_with(&root) {
            return Err(OutputFileError::TargetEscapesRoot {
                target: file.path.display().to_string(),
                root: root.clone(),
            });
        }
        if file.path.file_name().is_none() {
            return Err(OutputFileError::UnsafeTarget {
                target: file.path.display().to_string(),
            });
        }
        reject_symlink_components(&symlink_anchor, &file.path)?;

        let status = match owned_file_state(&file.path, &file.content_hash)? {
            OwnedFileState::Missing => OutputFileRemovalStatus::Missing,
            OwnedFileState::ExternalModified => OutputFileRemovalStatus::ExternalModified,
            OwnedFileState::Matches => match fs::remove_file(&file.path) {
                Ok(()) => OutputFileRemovalStatus::Removed,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    OutputFileRemovalStatus::Missing
                }
                Err(source) => {
                    return Err(OutputFileError::Io {
                        path: file.path.clone(),
                        source,
                    });
                }
            },
        };
        removed.push(RemovedOutputFile {
            route_key: file.route_key.clone(),
            path: file.path.clone(),
            status,
        });
    }

    Ok(removed)
}

pub fn render_plan_paths(
    plan: &RenderPlan,
    base_dir: &Path,
) -> Result<Vec<PlannedOutputFile>, OutputFileError> {
    let root = output_root(base_dir, &plan.output)?;
    let symlink_anchor = symlink_check_anchor(base_dir, &root, &plan.output);

    render_plan_paths_with_anchor(plan, base_dir, &root, &symlink_anchor)
}

fn render_plan_paths_with_anchor(
    plan: &RenderPlan,
    base_dir: &Path,
    root: &Path,
    symlink_anchor: &Path,
) -> Result<Vec<PlannedOutputFile>, OutputFileError> {
    let mut planned_files = Vec::with_capacity(plan.files.len());

    for file in &plan.files {
        let path = output_file_path(base_dir, root, &plan.output, &file.target)?;
        reject_symlink_components(symlink_anchor, &path)?;
        planned_files.push(PlannedOutputFile {
            route_key: file.route_key.clone(),
            target: file.target.clone(),
            path,
        });
    }

    Ok(planned_files)
}

fn short_hash(value: &str) -> String {
    let hash = value.chars().take(8).collect::<String>();

    if hash.is_empty() {
        String::from("00000000")
    } else {
        hash
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

fn content_hash(contents: &str) -> String {
    format!("{:016x}", stable_hash(contents.as_bytes()))
}

fn output_root(base_dir: &Path, output: &OutputContext) -> Result<PathBuf, OutputFileError> {
    if let Some(root) = output.root.as_deref() {
        return clean_root_path(base_dir, root);
    }

    let prefix = output.target.split("{{").next().unwrap_or_default();
    let directory_prefix = literal_directory_prefix(prefix);

    safe_join(base_dir, directory_prefix).map_err(|_| OutputFileError::UnsafeRoot {
        root: directory_prefix.display().to_string(),
    })
}

fn literal_directory_prefix(prefix: &str) -> &Path {
    let trimmed = prefix.trim_end_matches(['/', '\\']);

    if prefix.ends_with(['/', '\\']) {
        return Path::new(trimmed);
    }

    Path::new(trimmed).parent().unwrap_or_else(|| Path::new(""))
}

fn clean_root_path(base_dir: &Path, root: &str) -> Result<PathBuf, OutputFileError> {
    let root_path = Path::new(root);

    if root_path.is_absolute() {
        clean_components(root_path).map_err(|_| OutputFileError::UnsafeRoot {
            root: root.to_string(),
        })
    } else {
        safe_join(base_dir, root_path).map_err(|_| OutputFileError::UnsafeRoot {
            root: root.to_string(),
        })
    }
}

fn output_file_path(
    base_dir: &Path,
    root: &Path,
    output: &OutputContext,
    target: &str,
) -> Result<PathBuf, OutputFileError> {
    let target_path = Path::new(target);
    let path = if output.root.is_some() {
        safe_join(root, target_path)
    } else {
        safe_join(base_dir, target_path)
    }
    .map_err(|_| OutputFileError::UnsafeTarget {
        target: target.to_string(),
    })?;

    if !path.starts_with(root) {
        return Err(OutputFileError::TargetEscapesRoot {
            target: target.to_string(),
            root: root.to_path_buf(),
        });
    }
    if path.file_name().is_none() {
        return Err(OutputFileError::UnsafeTarget {
            target: target.to_string(),
        });
    }

    Ok(path)
}

fn symlink_check_anchor(base_dir: &Path, root: &Path, output: &OutputContext) -> PathBuf {
    match output.root.as_deref().map(Path::new) {
        Some(configured_root) if configured_root.is_absolute() => root.to_path_buf(),
        _ => base_dir.to_path_buf(),
    }
}

fn safe_join(base: &Path, relative: &Path) -> Result<PathBuf, ()> {
    if relative.is_absolute() {
        return Err(());
    }

    let mut path = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => path.push(value),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(()),
        }
    }

    Ok(path)
}

fn clean_components(path: &Path) -> Result<PathBuf, ()> {
    let mut clean = PathBuf::new();

    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                clean.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(()),
        }
    }

    Ok(clean)
}

fn reject_symlink_components(anchor: &Path, path: &Path) -> Result<(), OutputFileError> {
    let relative = path.strip_prefix(anchor).unwrap_or(path);
    let mut current = anchor.to_path_buf();

    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(OutputFileError::SymlinkInPath { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(OutputFileError::Io {
                    path: current,
                    source,
                });
            }
        }
    }

    Ok(())
}

fn verify_existing_target(
    path: &Path,
    owned_hashes: &BTreeMap<PathBuf, String>,
) -> Result<(), OutputFileError> {
    if !path.exists() {
        return Ok(());
    }
    let Some(expected_hash) = owned_hashes.get(path) else {
        return Err(OutputFileError::UnownedTarget {
            path: path.to_path_buf(),
        });
    };
    let contents = fs::read_to_string(path).map_err(|source| OutputFileError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    if content_hash(&contents) != *expected_hash {
        return Err(OutputFileError::ExternalModified {
            path: path.to_path_buf(),
        });
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnedFileState {
    Matches,
    Missing,
    ExternalModified,
}

fn owned_file_state(path: &Path, expected_hash: &str) -> Result<OwnedFileState, OutputFileError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(OwnedFileState::Missing);
        }
        Err(source) => {
            return Err(OutputFileError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if content_hash(&contents) != expected_hash {
        return Ok(OwnedFileState::ExternalModified);
    }

    Ok(OwnedFileState::Matches)
}

fn atomic_write(anchor: &Path, path: &Path, contents: &str) -> Result<(), OutputFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|source| OutputFileError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
        reject_symlink_components(anchor, parent)?;
    }

    let temp_path = temp_sibling(path);
    fs::write(&temp_path, contents).map_err(|source| OutputFileError::Io {
        path: temp_path.clone(),
        source,
    })?;
    fs::rename(&temp_path, path).map_err(|source| {
        let _ = fs::remove_file(&temp_path);
        OutputFileError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn temp_sibling(path: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");

    path.with_file_name(format!(
        ".{filename}.bindport-tmp-{}-{now}",
        std::process::id()
    ))
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
    use bindport_core::{OutputDeleteState, OutputFailurePolicy};
    use std::collections::BTreeMap;

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

    #[test]
    fn render_output_routes_builds_targets_and_context() {
        let mut vars = BTreeMap::new();
        vars.insert(String::from("mode"), serde_json::json!("dev"));
        let output = OutputRenderConfig::from(&EffectiveOutputConfig {
            name: String::from("debug"),
            template: String::from("debug-template"),
            root: Some(String::from(".bindport/generated")),
            target: String::from("debug/{{ route.slug }}.txt"),
            target_host: String::from("host.docker.internal"),
            target_scheme: String::from("https"),
            auto_render: true,
            delete_on: vec![OutputDeleteState::Removed],
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 250,
            vars,
        });
        let route = test_route("route-1", "active", Some("feature-tree.demo.localhost"));

        let plan = render_output_routes(
            &output,
            "target={{ route.target_url }} mode={{ vars.mode }} output={{ output.name }}",
            &[route],
        )
        .expect("render plan");

        assert_eq!(plan.output.name, "debug");
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].target, "debug/demo-web-feature-tree.txt");
        assert_eq!(
            plan.files[0].contents,
            "target=https://host.docker.internal:29100 mode=dev output=debug"
        );
        assert_eq!(
            plan.files[0].context.route.unique_slug,
            "demo-web-feature-tree-abc12345"
        );
        assert_eq!(plan.files[0].context.output.delete_on, vec!["removed"]);
    }

    #[test]
    fn render_output_routes_reports_target_collisions() {
        let output = OutputRenderConfig::from(&EffectiveOutputConfig {
            name: String::from("debug"),
            template: String::from("debug-template"),
            root: None,
            target: String::from("debug/{{ route.service }}.txt"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: vec![OutputDeleteState::Removed],
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 250,
            vars: BTreeMap::new(),
        });
        let first = test_route("route-1", "active", Some("first.demo.localhost"));
        let second = test_route("route-2", "active", Some("second.demo.localhost"));

        let error = render_output_routes(&output, "ok", &[first, second]).expect_err("collision");

        assert!(matches!(
            error,
            RenderError::TargetCollision { ref target, ref route_keys }
                if target == "debug/web.txt"
                    && route_keys == &vec![String::from("route-1"), String::from("route-2")]
        ));
    }

    #[test]
    fn built_in_traefik_plan_renders_comment_for_stopped_route() {
        let template = TemplateResolver::new(None, None)
            .resolve("bindport-traefik", None)
            .expect("built-in template");
        let output = OutputRenderConfig::from(&EffectiveOutputConfig {
            name: String::from("traefik"),
            template: String::from("bindport-traefik"),
            root: None,
            target: String::from("traefik/{{ route.slug }}.yml"),
            target_host: String::from("127.0.0.1"),
            target_scheme: String::from("http"),
            auto_render: true,
            delete_on: vec![OutputDeleteState::Removed],
            on_failure: OutputFailurePolicy::Warn,
            debounce_ms: 250,
            vars: BTreeMap::new(),
        });
        let route = test_route("route-1", "stopped", Some("feature-tree.demo.localhost"));

        let plan = render_output_routes(&output, &template.contents, &[route]).expect("plan");

        assert_eq!(plan.files[0].target, "traefik/demo-web-feature-tree.yml");
        assert!(plan.files[0].contents.contains("is stopped"));
        assert!(!plan.files[0].contents.contains("routers:"));
    }

    #[test]
    fn write_render_plan_writes_new_files_under_root() {
        let root = temp_test_dir("write-plan-new");
        let plan = test_render_plan("routes/demo.yml", "first");

        let written = write_render_plan(&plan, &root, &[]).expect("write plan");

        assert_eq!(written.len(), 1);
        assert_eq!(written[0].path, root.join(".bindport/out/routes/demo.yml"));
        assert_eq!(
            fs::read_to_string(&written[0].path).expect("rendered file"),
            "first"
        );
        assert_eq!(written[0].content_hash, content_hash("first"));
    }

    #[cfg(unix)]
    #[test]
    fn write_render_plan_allows_symlinked_base_directory() {
        let real_root = temp_test_dir("write-plan-real-base");
        let link_parent = temp_test_dir("write-plan-link-parent");
        let link_root = link_parent.join("base-link");
        std::os::unix::fs::symlink(&real_root, &link_root).expect("symlink base dir");
        let plan = test_render_plan("routes/demo.yml", "first");

        let written = write_render_plan(&plan, &link_root, &[]).expect("write plan");

        assert_eq!(
            fs::read_to_string(&written[0].path).expect("rendered file"),
            "first"
        );
        assert_eq!(
            written[0].path,
            link_root.join(".bindport/out/routes/demo.yml")
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_render_plan_rejects_symlink_below_output_root() {
        let root = temp_test_dir("write-plan-symlink-target");
        let outside = temp_test_dir("write-plan-symlink-outside");
        let symlink_path = root.join(".bindport/out/routes");
        fs::create_dir_all(symlink_path.parent().expect("parent")).expect("parent dir");
        std::os::unix::fs::symlink(&outside, &symlink_path).expect("symlink target dir");
        let plan = test_render_plan("routes/demo.yml", "first");

        let error = write_render_plan(&plan, &root, &[]).expect_err("symlink below root");

        assert!(matches!(
            error,
            OutputFileError::SymlinkInPath { path } if path == symlink_path
        ));
    }

    #[test]
    fn write_render_plan_refuses_unowned_existing_file() {
        let root = temp_test_dir("write-plan-unowned");
        let path = root.join(".bindport/out/routes/demo.yml");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        fs::write(&path, "external").expect("external file");
        let plan = test_render_plan("routes/demo.yml", "first");

        let error = write_render_plan(&plan, &root, &[]).expect_err("unowned file");

        assert!(matches!(
            error,
            OutputFileError::UnownedTarget { path: error_path } if error_path == path
        ));
    }

    #[test]
    fn write_render_plan_overwrites_owned_file_when_hash_matches() {
        let root = temp_test_dir("write-plan-owned");
        let path = root.join(".bindport/out/routes/demo.yml");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        fs::write(&path, "old").expect("old file");
        let plan = test_render_plan("routes/demo.yml", "new");

        let written = write_render_plan(
            &plan,
            &root,
            &[OutputFileOwnership {
                path: path.clone(),
                content_hash: content_hash("old"),
            }],
        )
        .expect("overwrite owned file");

        assert_eq!(written[0].content_hash, content_hash("new"));
        assert_eq!(fs::read_to_string(&path).expect("rendered file"), "new");
    }

    #[test]
    fn write_render_plan_refuses_externally_modified_owned_file() {
        let root = temp_test_dir("write-plan-modified");
        let path = root.join(".bindport/out/routes/demo.yml");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        fs::write(&path, "changed").expect("changed file");
        let plan = test_render_plan("routes/demo.yml", "new");

        let error = write_render_plan(
            &plan,
            &root,
            &[OutputFileOwnership {
                path: path.clone(),
                content_hash: content_hash("old"),
            }],
        )
        .expect_err("externally modified file");

        assert!(matches!(
            error,
            OutputFileError::ExternalModified { path: error_path } if error_path == path
        ));
    }

    #[test]
    fn remove_owned_output_files_deletes_matching_files() {
        let root = temp_test_dir("remove-owned");
        let output = test_render_plan("routes/demo.yml", "owned").output;
        let path = root.join(".bindport/out/routes/demo.yml");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        fs::write(&path, "owned").expect("owned file");

        let removed = remove_owned_output_files(
            &[RemovableOutputFile {
                route_key: String::from("route-1"),
                path: path.clone(),
                content_hash: content_hash("owned"),
            }],
            &root,
            &output,
        )
        .expect("remove owned file");

        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].route_key, "route-1");
        assert_eq!(removed[0].path, path);
        assert_eq!(removed[0].status, OutputFileRemovalStatus::Removed);
        assert!(!removed[0].path.exists());
    }

    #[test]
    fn remove_owned_output_files_reports_missing_files() {
        let root = temp_test_dir("remove-missing");
        let output = test_render_plan("routes/missing.yml", "owned").output;
        let path = root.join(".bindport/out/routes/missing.yml");

        let removed = remove_owned_output_files(
            &[RemovableOutputFile {
                route_key: String::from("route-1"),
                path: path.clone(),
                content_hash: content_hash("owned"),
            }],
            &root,
            &output,
        )
        .expect("remove missing file");

        assert_eq!(removed[0].status, OutputFileRemovalStatus::Missing);
        assert_eq!(removed[0].path, path);
    }

    #[test]
    fn remove_owned_output_files_preserves_externally_modified_files() {
        let root = temp_test_dir("remove-modified");
        let output = test_render_plan("routes/demo.yml", "owned").output;
        let path = root.join(".bindport/out/routes/demo.yml");
        fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        fs::write(&path, "changed").expect("changed file");

        let removed = remove_owned_output_files(
            &[RemovableOutputFile {
                route_key: String::from("route-1"),
                path: path.clone(),
                content_hash: content_hash("owned"),
            }],
            &root,
            &output,
        )
        .expect("check modified file");

        assert_eq!(removed[0].status, OutputFileRemovalStatus::ExternalModified);
        assert_eq!(
            fs::read_to_string(&path).expect("preserved file"),
            "changed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_owned_output_files_rejects_symlink_below_output_root() {
        let root = temp_test_dir("remove-symlink-root");
        let outside = temp_test_dir("remove-symlink-outside");
        let output = test_render_plan("routes/demo.yml", "owned").output;
        let routes_dir = root.join(".bindport/out/routes");
        let path = routes_dir.join("demo.yml");
        let outside_path = outside.join("demo.yml");
        fs::create_dir_all(&routes_dir).expect("routes dir");
        fs::write(&path, "owned").expect("owned file");
        fs::remove_file(&path).expect("remove original file");
        fs::remove_dir(&routes_dir).expect("remove original dir");
        fs::write(&outside_path, "owned").expect("outside file");
        std::os::unix::fs::symlink(&outside, &routes_dir).expect("symlink routes dir");

        let error = remove_owned_output_files(
            &[RemovableOutputFile {
                route_key: String::from("route-1"),
                path: path.clone(),
                content_hash: content_hash("owned"),
            }],
            &root,
            &output,
        )
        .expect_err("symlink below root");

        assert!(matches!(
            error,
            OutputFileError::SymlinkInPath { path: error_path } if error_path == routes_dir
        ));
        assert!(outside_path.is_file());
    }

    #[test]
    fn write_render_plan_rejects_targets_that_escape_root() {
        let root = temp_test_dir("write-plan-escape");
        let plan = test_render_plan("../demo.yml", "escape");

        let error = write_render_plan(&plan, &root, &[]).expect_err("unsafe target");

        assert!(matches!(error, OutputFileError::UnsafeTarget { .. }));
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

    fn test_render_plan(target: &str, contents: &str) -> RenderPlan {
        RenderPlan {
            output: OutputContext {
                name: String::from("debug"),
                template: String::from("debug-template"),
                root: Some(String::from(".bindport/out")),
                target: String::from("routes/{{ route.slug }}.yml"),
                auto_render: true,
                delete_on: vec![String::from("removed")],
                on_failure: String::from("warn"),
            },
            files: vec![RenderedRouteFile {
                route_key: String::from("route-1"),
                target: target.to_string(),
                contents: contents.to_string(),
                context: RenderContext {
                    route: RouteContext {
                        key: String::from("route-1"),
                        project: String::from("demo"),
                        service: String::from("web"),
                        state: String::from("active"),
                        health: String::from("unknown"),
                        port: 29_100,
                        host: String::from("127.0.0.1"),
                        url: String::from("http://127.0.0.1:29100"),
                        hostname: Some(String::from("demo.localhost")),
                        route_url: Some(String::from("http://demo.localhost")),
                        target_url: String::from("http://127.0.0.1:29100"),
                        branch: Some(String::from("feature/tree")),
                        branch_label: Some(String::from("feature-tree")),
                        worktree_path: Some(String::from("/workspace/demo-feature-tree")),
                        worktree_label: String::from("demo-feature-tree"),
                        worktree_hash: Some(String::from("abc123456789")),
                        slug: String::from("demo-web-feature-tree"),
                        unique_slug: String::from("demo-web-feature-tree-abc12345"),
                        pid: Some(12_345),
                        command: String::from("next dev"),
                        cwd: String::from("/workspace/demo-feature-tree"),
                        started_at: String::from("2026-06-29T00:00:00Z"),
                        updated_at: String::from("2026-06-29T00:01:00Z"),
                    },
                    output: OutputContext {
                        name: String::from("debug"),
                        template: String::from("debug-template"),
                        root: Some(String::from(".bindport/out")),
                        target: String::from("routes/{{ route.slug }}.yml"),
                        auto_render: true,
                        delete_on: vec![String::from("removed")],
                        on_failure: String::from("warn"),
                    },
                    vars: BTreeMap::new(),
                },
            }],
        }
    }

    fn test_route(key: &str, state: &str, hostname: Option<&str>) -> RouteRecord {
        RouteRecord {
            key: key.to_string(),
            project: String::from("demo"),
            service: String::from("web"),
            state: state.to_string(),
            health: String::from("unknown"),
            port: 29_100,
            host: String::from("127.0.0.1"),
            url: String::from("http://127.0.0.1:29100"),
            hostname: hostname.map(str::to_string),
            route_url: hostname.map(|hostname| format!("http://{hostname}")),
            branch: Some(String::from("feature/tree")),
            branch_label: Some(String::from("feature-tree")),
            worktree_path: Some(String::from("/workspace/demo-feature-tree")),
            worktree_hash: Some(String::from("abc123456789")),
            pid: Some(12_345),
            command: String::from("next dev"),
            cwd: String::from("/workspace/demo-feature-tree"),
            started_at: String::from("2026-06-29T00:00:00Z"),
            updated_at: String::from("2026-06-29T00:01:00Z"),
        }
    }
}
