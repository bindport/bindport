use super::*;

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
    pub(crate) fn context(&self, output: &OutputRenderConfig) -> RouteContext {
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
#[derive(Debug)]
pub enum RenderError {
    UnsafeHostname {
        route_key: String,
        hostname: String,
    },
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

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeHostname {
                route_key,
                hostname,
            } => {
                write!(f, "route `{route_key}` has unsafe hostname `{hostname}`")
            }
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
            Self::UnsafeHostname { .. } | Self::TargetCollision { .. } => None,
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
        if let Some(hostname) = route.hostname.as_deref()
            && hostname.contains('`')
        {
            return Err(RenderError::UnsafeHostname {
                route_key: route.key.clone(),
                hostname: hostname.to_string(),
            });
        }
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
