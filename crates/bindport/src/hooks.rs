use super::*;

#[derive(Debug, Clone)]
pub(crate) struct EffectiveHook {
    pub(crate) name: String,
    pub(crate) events: Vec<HookEvent>,
    pub(crate) command: Vec<String>,
    pub(crate) timeout: Duration,
    pub(crate) timeout_ms: u64,
    pub(crate) source: String,
    pub(crate) definition: String,
    pub(crate) hook_hash: String,
    pub(crate) target: HookTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTrustScope {
    Worktree,
    Repo,
}

impl HookTrustScope {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Repo => "repo",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "worktree" => Some(Self::Worktree),
            "repo" => Some(Self::Repo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HookPlan {
    pub(crate) hooks: Vec<EffectiveHook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTarget {
    pub(crate) kind: HookTargetKind,
    pub(crate) display: String,
    pub(crate) fingerprint: String,
    pub(crate) hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTargetKind {
    LocalFile,
    MissingFile,
    Opaque,
}

impl HookTargetKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::LocalFile => "local_file",
            Self::MissingFile => "missing_file",
            Self::Opaque => "opaque",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookDecision {
    Approved,
    Denied,
}

impl HookDecision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "approved" => Some(Self::Approved),
            "denied" => Some(Self::Denied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookTrustStatus {
    Approved { scope: HookTrustScope },
    Denied { scope: HookTrustScope },
    Changed,
    Pending,
}

impl HookTrustStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Approved { .. } => "approved",
            Self::Denied { .. } => "denied",
            Self::Changed => "changed",
            Self::Pending => "pending",
        }
    }

    pub(crate) const fn is_approved(self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct HookStatus {
    pub(crate) hook: EffectiveHook,
    pub(crate) trust: HookTrustStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookRunMode {
    Run,
    DryRun,
}

#[derive(Debug)]
pub(crate) enum HookExecutionError {
    Spawn { command: String, source: io::Error },
    Wait { command: String, source: io::Error },
    Timeout { command: String, timeout: Duration },
    Failed { command: String, status: ExitStatus },
}

impl std::fmt::Display for HookExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn { command, source } => {
                write!(f, "failed to spawn hook `{command}`: {source}")
            }
            Self::Wait { command, source } => {
                write!(f, "failed waiting for hook `{command}`: {source}")
            }
            Self::Timeout { command, timeout } => {
                write!(
                    f,
                    "hook `{command}` timed out after {}ms",
                    timeout.as_millis()
                )
            }
            Self::Failed { command, status } => {
                write!(f, "hook `{command}` exited with {status}")
            }
        }
    }
}

impl std::error::Error for HookExecutionError {}

pub(crate) fn configured_hook_plan(cwd: &Path, config: &ResolvedConfig) -> Option<HookPlan> {
    let loaded = config.loaded.as_ref()?;
    let hooks = loaded.config.hooks.as_ref()?;
    let commands = hooks.commands.as_deref().unwrap_or_default();
    let source = hook_command_source(config);
    let default_timeout = hooks.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS);
    let hooks = commands
        .iter()
        .enumerate()
        .filter(|(_, hook)| hook.enabled.unwrap_or(true))
        .filter_map(|(index, hook)| effective_hook(cwd, index, hook, default_timeout, &source))
        .collect::<Vec<_>>();

    Some(HookPlan { hooks })
}

pub(crate) fn effective_hook(
    cwd: &Path,
    index: usize,
    hook: &HookCommandConfig,
    default_timeout_ms: u64,
    source: &str,
) -> Option<EffectiveHook> {
    let command = hook.command.clone()?;
    let events = hook.events.clone()?;
    let timeout_ms = hook.timeout_ms.unwrap_or(default_timeout_ms);
    let name = hook
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("hook-{}", index + 1));
    let target = hook_target(cwd, &command);
    let definition = hook_definition(&name, &events, &command, timeout_ms, source);
    let hook_hash = stable_hex_hash(definition.as_bytes());

    Some(EffectiveHook {
        name,
        events,
        command,
        timeout: Duration::from_millis(timeout_ms),
        timeout_ms,
        source: source.to_string(),
        definition,
        hook_hash,
        target,
    })
}

pub(crate) fn hook_command_source(config: &ResolvedConfig) -> String {
    let Some(loaded) = config.loaded.as_ref() else {
        return String::from("unknown config");
    };

    if let Some(local) = loaded.local_override.as_ref()
        && local
            .config
            .hooks
            .as_ref()
            .and_then(|hooks| hooks.commands.as_ref())
            .is_some()
    {
        return format!("local override config `{}`", local.path.display());
    }

    format!(
        "{} config `{}`",
        loaded.source.as_str(),
        loaded.path.display()
    )
}

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

pub(crate) fn hook_target(cwd: &Path, command: &[String]) -> HookTarget {
    let Some(program) = command.first().map(String::as_str) else {
        return opaque_hook_target("<empty>");
    };

    if !path_like_command(program) {
        return opaque_hook_target(program);
    }

    let path = PathBuf::from(program);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let display_path = path.display().to_string();

    match fs::read(&path) {
        Ok(contents) => {
            let resolved = path
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(&path));
            let fingerprint = format!(
                "file:{}:{}:{}",
                program,
                contents.len(),
                stable_hex_hash(&contents)
            );
            HookTarget {
                kind: HookTargetKind::LocalFile,
                display: resolved.display().to_string(),
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
        Err(_) => {
            let fingerprint = format!("missing:{program}");
            HookTarget {
                kind: HookTargetKind::MissingFile,
                display: display_path,
                hash: stable_hex_hash(fingerprint.as_bytes()),
                fingerprint,
            }
        }
    }
}

pub(crate) fn opaque_hook_target(program: &str) -> HookTarget {
    let fingerprint = format!("opaque:{program}");
    HookTarget {
        kind: HookTargetKind::Opaque,
        display: program.to_string(),
        hash: stable_hex_hash(fingerprint.as_bytes()),
        fingerprint,
    }
}

pub(crate) fn path_like_command(program: &str) -> bool {
    program.contains('/') || program.contains('\\') || program.starts_with('.')
}

pub(crate) fn path_clean_display_path(path: &Path) -> PathBuf {
    path.components().collect()
}

pub(crate) fn stable_hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HookTrustStore {
    pub(crate) entries: Vec<HookTrustEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTrustEntry {
    pub(crate) subject: String,
    pub(crate) scope: HookTrustScope,
    pub(crate) name: String,
    pub(crate) decision: HookDecision,
    pub(crate) definition: String,
    pub(crate) target: String,
    pub(crate) hook_hash: String,
    pub(crate) target_hash: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct HookTrustSubjects {
    pub(crate) worktree: String,
    pub(crate) repo: Option<String>,
}

impl HookTrustSubjects {
    pub(crate) fn subject(&self, scope: HookTrustScope) -> Option<&str> {
        match scope {
            HookTrustScope::Worktree => Some(&self.worktree),
            HookTrustScope::Repo => self.repo.as_deref(),
        }
    }
}

pub(crate) fn hook_trust_subjects(cwd: &Path) -> HookTrustSubjects {
    match detect_git_identity(cwd) {
        Some(git) => HookTrustSubjects {
            worktree: format!("worktree:{}", git.worktree_path.display()),
            repo: Some(format!("repo:{}", git.git_common_dir.display())),
        },
        None => {
            let path = cwd
                .canonicalize()
                .unwrap_or_else(|_| path_clean_display_path(cwd));
            HookTrustSubjects {
                worktree: format!("path:{}", path.display()),
                repo: None,
            }
        }
    }
}

pub(crate) fn read_hook_trust_store() -> io::Result<HookTrustStore> {
    let path = hook_trust_path()?;
    if !path.is_file() {
        return Ok(HookTrustStore::default());
    }

    let contents = fs::read_to_string(path)?;
    let value = serde_json::from_str::<serde_json::Value>(&contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let entries = value
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(|entries| entries.iter().filter_map(parse_hook_trust_entry).collect())
        .unwrap_or_default();

    Ok(HookTrustStore { entries })
}

pub(crate) fn parse_hook_trust_entry(value: &serde_json::Value) -> Option<HookTrustEntry> {
    Some(HookTrustEntry {
        subject: value.get("subject")?.as_str()?.to_string(),
        scope: HookTrustScope::parse(value.get("scope")?.as_str()?)?,
        name: value.get("name")?.as_str()?.to_string(),
        decision: HookDecision::parse(value.get("decision")?.as_str()?)?,
        definition: value.get("definition")?.as_str()?.to_string(),
        target: value.get("target")?.as_str()?.to_string(),
        hook_hash: value.get("hook_hash")?.as_str()?.to_string(),
        target_hash: value.get("target_hash")?.as_str()?.to_string(),
        updated_at: value.get("updated_at")?.as_str()?.to_string(),
    })
}

pub(crate) fn write_hook_trust_store(store: &HookTrustStore) -> io::Result<()> {
    let path = hook_trust_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let entries = store
        .entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "subject": entry.subject,
                "scope": entry.scope.as_str(),
                "name": entry.name,
                "decision": entry.decision.as_str(),
                "definition": entry.definition,
                "target": entry.target,
                "hook_hash": entry.hook_hash,
                "target_hash": entry.target_hash,
                "updated_at": entry.updated_at,
            })
        })
        .collect::<Vec<_>>();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": HOOK_TRUST_SCHEMA_VERSION,
        "entries": entries,
    }))
    .map_err(io::Error::other)?;

    fs::write(path, format!("{json}\n"))
}

pub(crate) fn hook_trust_status(
    hook: &EffectiveHook,
    store: &HookTrustStore,
    subjects: &HookTrustSubjects,
) -> HookTrustStatus {
    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if let Some(entry) = store.entries.iter().find(|entry| {
            entry.scope == scope
                && entry.subject == subject
                && entry.name == hook.name
                && entry.definition == hook.definition
                && entry.target == hook.target.fingerprint
        }) {
            return match entry.decision {
                HookDecision::Approved => HookTrustStatus::Approved { scope },
                HookDecision::Denied => HookTrustStatus::Denied { scope },
            };
        }
    }

    for scope in [HookTrustScope::Worktree, HookTrustScope::Repo] {
        let Some(subject) = subjects.subject(scope) else {
            continue;
        };
        if store.entries.iter().any(|entry| {
            entry.scope == scope && entry.subject == subject && entry.name == hook.name
        }) {
            return HookTrustStatus::Changed;
        }
    }

    HookTrustStatus::Pending
}

pub(crate) fn hook_statuses_for_current_dir(
    cwd: &Path,
    config: &ResolvedConfig,
) -> Vec<HookStatus> {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return Vec::new();
    };
    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    plan.hooks
        .into_iter()
        .map(|hook| {
            let trust = hook_trust_status(&hook, &store, &subjects);
            HookStatus { hook, trust }
        })
        .collect()
}

pub(crate) fn upsert_hook_trust_entry(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    hook: &EffectiveHook,
    decision: HookDecision,
) -> Result<(), String> {
    let Some(subject) = subjects.subject(scope) else {
        return Err(String::from(
            "repo scope is only available inside a git repository",
        ));
    };
    store.entries.retain(|entry| {
        !(entry.scope == scope && entry.subject == subject && entry.name == hook.name)
    });
    store.entries.push(HookTrustEntry {
        subject: subject.to_string(),
        scope,
        name: hook.name.clone(),
        decision,
        definition: hook.definition.clone(),
        target: hook.target.fingerprint.clone(),
        hook_hash: hook.hook_hash.clone(),
        target_hash: hook.target.hash.clone(),
        updated_at: unix_timestamp_string(),
    });

    Ok(())
}

pub(crate) fn reset_hook_trust_entries(
    store: &mut HookTrustStore,
    subjects: &HookTrustSubjects,
    scope: HookTrustScope,
    names: &BTreeSet<String>,
) -> usize {
    let Some(subject) = subjects.subject(scope) else {
        return 0;
    };
    let before = store.entries.len();
    store.entries.retain(|entry| {
        !(entry.scope == scope
            && entry.subject == subject
            && (names.is_empty() || names.contains(&entry.name)))
    });

    before - store.entries.len()
}

pub(crate) fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| String::from("0"))
}

pub(crate) fn run_hooks_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    events: &RouteEventCollector,
    output_rendered: bool,
    mode: HookRunMode,
) -> usize {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        return 0;
    };
    let hook_events = events.hook_events(output_rendered);
    if hook_events.is_empty() {
        return 0;
    }
    let matching_hooks = plan
        .hooks
        .iter()
        .filter(|hook| hook_matches_events(hook, &hook_events))
        .collect::<Vec<_>>();

    if matching_hooks.is_empty() {
        return 0;
    }

    let store = match read_hook_trust_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("bindport: warning: hook trust store unavailable: {error}");
            return 0;
        }
    };
    let subjects = hook_trust_subjects(cwd);
    let env = HookEnvironment::new(events, &hook_events);
    let mut ran = 0;
    for hook in &matching_hooks {
        let trust = hook_trust_status(hook, &store, &subjects);
        if !trust.is_approved() {
            print_hook_not_trusted_warning(hook, trust);
            continue;
        }

        match mode {
            HookRunMode::DryRun => print_hook_dry_run(hook),
            HookRunMode::Run => {
                if let Err(error) = execute_hook(cwd, hook, &env) {
                    eprintln!("bindport: warning: {error}");
                }
            }
        }
        ran += 1;
    }

    ran
}

pub(crate) fn hook_matches_events(hook: &EffectiveHook, events: &BTreeSet<HookEvent>) -> bool {
    hook.events.iter().any(|event| events.contains(event))
}

pub(crate) fn print_hook_not_trusted_warning(hook: &EffectiveHook, trust: HookTrustStatus) {
    let reason = match trust {
        HookTrustStatus::Pending => "pending approval",
        HookTrustStatus::Changed => "changed since the last trust decision",
        HookTrustStatus::Denied { .. } => "denied",
        HookTrustStatus::Approved { .. } => return,
    };
    eprintln!(
        "bindport: warning: hook `{}` not run ({reason}); inspect with `bindport hooks status`",
        hook.name
    );
}

#[derive(Debug)]
pub(crate) struct HookEnvironment {
    pub(crate) events: String,
    pub(crate) sources: String,
    pub(crate) context: String,
}

impl HookEnvironment {
    pub(crate) fn new(
        route_events: &RouteEventCollector,
        hook_events: &BTreeSet<HookEvent>,
    ) -> Self {
        Self {
            events: hook_events
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(","),
            sources: route_events.hook_sources(),
            context: route_events.warning_context(),
        }
    }
}

pub(crate) fn print_hook_dry_run(hook: &EffectiveHook) {
    println!(
        "would run hook {} ({}): {}",
        hook.name,
        hook.source,
        command_display(&hook.command)
    );
    println!(
        "  env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
    );
}

pub(crate) fn execute_hook(
    cwd: &Path,
    hook: &EffectiveHook,
    env: &HookEnvironment,
) -> Result<(), HookExecutionError> {
    let Some((program, args)) = hook.command.split_first() else {
        return Err(HookExecutionError::Spawn {
            command: command_display(&hook.command),
            source: io::Error::new(io::ErrorKind::InvalidInput, "empty hook command"),
        });
    };
    let display = command_display(&hook.command);
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env_clear()
        .env("BINDPORT_HOOK_EVENTS", &env.events)
        .env("BINDPORT_HOOK_SOURCES", &env.sources)
        .env("BINDPORT_HOOK_CONTEXT", &env.context);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    configure_hook_command(&mut command);

    let mut child = command
        .spawn()
        .map_err(|source| HookExecutionError::Spawn {
            command: display.clone(),
            source,
        })?;
    let deadline = Instant::now() + hook.timeout;

    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(HookExecutionError::Failed {
                    command: display,
                    status,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_hook_child(&mut child);
                let _ = child.wait();
                return Err(HookExecutionError::Timeout {
                    command: display,
                    timeout: hook.timeout,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(source) => {
                return Err(HookExecutionError::Wait {
                    command: display,
                    source,
                });
            }
        }
    }
}

#[cfg(unix)]
pub(crate) fn configure_hook_command(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
pub(crate) fn configure_hook_command(_command: &mut Command) {}

#[cfg(unix)]
pub(crate) fn kill_hook_child(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    if pgid > 0 {
        let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
pub(crate) fn kill_hook_child(child: &mut Child) {
    let _ = child.kill();
}

pub(crate) fn command_display(command: &[String]) -> String {
    if command.is_empty() {
        String::from("<empty>")
    } else {
        command.join(" ")
    }
}
pub(crate) fn run_hooks_command(args: &[String]) -> ExitCode {
    match run_hooks_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(HooksCommandError::Config(error)) => {
            print_config_error(&error);
            ExitCode::FAILURE
        }
        Err(HooksCommandError::Io(error)) => {
            eprintln!("bindport: {error}");
            ExitCode::FAILURE
        }
        Err(HooksCommandError::InvalidArgument(message)) => {
            eprintln!("bindport: {message}");
            eprintln!("usage: bindport hooks status|trust|deny|reset [options]");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_hooks_command_result(args: &[String]) -> Result<(), HooksCommandError> {
    let options = parse_hooks_command(args)?;
    if options.command == HooksCommand::Help {
        print_hooks_help();
        return Ok(());
    }

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = resolve_config(&cwd)?;
    let Some(plan) = configured_hook_plan(&cwd, &config) else {
        println!("No hooks configured.");
        return Ok(());
    };
    if plan.hooks.is_empty() {
        println!("No enabled hooks configured.");
        return Ok(());
    }

    match options.command {
        HooksCommand::Status => print_hooks_status(&cwd, &config),
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset => {
            update_hook_trust(&cwd, plan.hooks, &options)
        }
        HooksCommand::Help => Ok(()),
    }
}

pub(crate) fn print_hooks_status(
    cwd: &Path,
    config: &ResolvedConfig,
) -> Result<(), HooksCommandError> {
    let statuses = hook_statuses_for_current_dir(cwd, config);
    if statuses.is_empty() {
        println!("No hooks configured.");
        return Ok(());
    }

    println!("BindPort hooks");
    for status in statuses {
        print_hook_status(&status);
    }

    Ok(())
}

pub(crate) fn print_hook_status(status: &HookStatus) {
    println!(
        "{}\t{}\t{}",
        status.trust.as_str(),
        status.hook.name,
        command_display(&status.hook.command)
    );
    println!("  trust: {}", hook_trust_status_display(status.trust));
    println!("  source: {}", status.hook.source);
    println!("  events: {}", hook_events_display(&status.hook.events));
    println!(
        "  target: {} ({})",
        status.hook.target.display,
        status.hook.target.kind.as_str()
    );
    println!("  hook hash: {}", status.hook.hook_hash);
    println!("  target hash: {}", status.hook.target.hash);
}

pub(crate) fn update_hook_trust(
    cwd: &Path,
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<(), HooksCommandError> {
    let selected = selected_hooks(hooks, options)?;
    let subjects = hook_trust_subjects(cwd);
    let mut store = read_hook_trust_store()?;
    let names = selected
        .iter()
        .map(|hook| hook.name.clone())
        .collect::<BTreeSet<_>>();

    match options.command {
        HooksCommand::Trust | HooksCommand::Deny => {
            let decision = if options.command == HooksCommand::Trust {
                HookDecision::Approved
            } else {
                HookDecision::Denied
            };
            for hook in &selected {
                upsert_hook_trust_entry(&mut store, &subjects, options.scope, hook, decision)
                    .map_err(HooksCommandError::InvalidArgument)?;
            }
            write_hook_trust_store(&store)?;
            println!(
                "{} {} hook(s) for {} scope",
                decision.as_str(),
                selected.len(),
                options.scope.as_str()
            );
        }
        HooksCommand::Reset => {
            let removed = reset_hook_trust_entries(&mut store, &subjects, options.scope, &names);
            write_hook_trust_store(&store)?;
            println!(
                "reset {removed} hook trust entr{} for {} scope",
                if removed == 1 { "y" } else { "ies" },
                options.scope.as_str()
            );
        }
        HooksCommand::Status | HooksCommand::Help => {}
    }

    Ok(())
}

pub(crate) fn selected_hooks(
    hooks: Vec<EffectiveHook>,
    options: &HooksCommandOptions,
) -> Result<Vec<EffectiveHook>, HooksCommandError> {
    if options.all {
        return Ok(hooks);
    }
    let Some(name) = options.name.as_deref() else {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    };
    let selected = hooks
        .into_iter()
        .filter(|hook| hook.name == name)
        .collect::<Vec<_>>();

    if selected.is_empty() {
        Err(HooksCommandError::InvalidArgument(format!(
            "hook `{name}` is not configured or is disabled"
        )))
    } else {
        Ok(selected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HooksCommand {
    Status,
    Trust,
    Deny,
    Reset,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HooksCommandOptions {
    pub(crate) command: HooksCommand,
    pub(crate) scope: HookTrustScope,
    pub(crate) all: bool,
    pub(crate) name: Option<String>,
}

pub(crate) fn parse_hooks_command(
    args: &[String],
) -> Result<HooksCommandOptions, HooksCommandError> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(HooksCommandOptions {
            command: HooksCommand::Status,
            scope: HookTrustScope::Worktree,
            all: false,
            name: None,
        });
    };
    let command = match command {
        "status" => HooksCommand::Status,
        "trust" => HooksCommand::Trust,
        "deny" => HooksCommand::Deny,
        "reset" => HooksCommand::Reset,
        "--help" | "-h" | "help" => HooksCommand::Help,
        unknown => {
            return Err(HooksCommandError::InvalidArgument(format!(
                "unknown hooks command `{unknown}`"
            )));
        }
    };

    let mut options = HooksCommandOptions {
        command,
        scope: HookTrustScope::Worktree,
        all: false,
        name: None,
    };
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--scope" => {
                index += 1;
                let Some(scope) = args.get(index).map(String::as_str) else {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "--scope requires worktree or repo",
                    )));
                };
                options.scope = HookTrustScope::parse(scope).ok_or_else(|| {
                    HooksCommandError::InvalidArgument(format!(
                        "invalid hook trust scope `{scope}`"
                    ))
                })?;
            }
            "--all" => options.all = true,
            "--help" | "-h" => {
                options.command = HooksCommand::Help;
            }
            value if value.starts_with('-') => {
                return Err(HooksCommandError::InvalidArgument(format!(
                    "unknown hooks option `{value}`"
                )));
            }
            value => {
                if options.name.is_some() {
                    return Err(HooksCommandError::InvalidArgument(String::from(
                        "only one hook name can be provided",
                    )));
                }
                options.name = Some(value.to_string());
            }
        }
        index += 1;
    }

    if options.all && options.name.is_some() {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "use either --all or a hook name, not both",
        )));
    }
    if matches!(
        options.command,
        HooksCommand::Trust | HooksCommand::Deny | HooksCommand::Reset
    ) && !options.all
        && options.name.is_none()
    {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hook name or --all is required",
        )));
    }
    if options.command == HooksCommand::Status && (options.all || options.name.is_some()) {
        return Err(HooksCommandError::InvalidArgument(String::from(
            "hooks status does not take a hook selector",
        )));
    }

    Ok(options)
}

#[derive(Debug)]
pub(crate) enum HooksCommandError {
    Config(ConfigError),
    Io(io::Error),
    InvalidArgument(String),
}

impl From<ConfigError> for HooksCommandError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<io::Error> for HooksCommandError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn hooks_status_json_for_current_dir() -> serde_json::Value {
    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    match resolve_config(&cwd) {
        Ok(config) => hooks_status_json(&cwd, &config),
        Err(error) => serde_json::json!({
            "error": error.to_string(),
            "items": [],
        }),
    }
}

pub(crate) fn hooks_status_json(cwd: &Path, config: &ResolvedConfig) -> serde_json::Value {
    let items = hook_statuses_for_current_dir(cwd, config)
        .into_iter()
        .map(|status| {
            serde_json::json!({
                "name": status.hook.name,
                "status": status.trust.as_str(),
                "trust": hook_trust_status_display(status.trust),
                "source": status.hook.source,
                "events": status
                    .hook
                    .events
                    .iter()
                    .map(|event| event.as_str())
                    .collect::<Vec<_>>(),
                "command": status.hook.command,
                "command_display": command_display(&status.hook.command),
                "timeout_ms": status.hook.timeout_ms,
                "hook_hash": status.hook.hook_hash,
                "target": {
                    "kind": status.hook.target.kind.as_str(),
                    "display": status.hook.target.display,
                    "hash": status.hook.target.hash,
                },
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({ "items": items })
}
pub(crate) fn print_doctor_hooks(cwd: &Path, config: &ResolvedConfig) {
    let Some(plan) = configured_hook_plan(cwd, config) else {
        println!("hooks: none configured");
        return;
    };

    if plan.hooks.is_empty() {
        println!("hooks: none enabled");
        return;
    }

    let store = read_hook_trust_store().unwrap_or_default();
    let subjects = hook_trust_subjects(cwd);

    println!("hooks: {} configured", plan.hooks.len());
    for hook in plan.hooks {
        let trust = hook_trust_status(&hook, &store, &subjects);
        println!("  hook {}:", hook.name);
        println!("    trust: {}", hook_trust_status_display(trust));
        println!("    source: {}", hook.source);
        println!("    events: {}", hook_events_display(&hook.events));
        println!("    command: {}", command_display(&hook.command));
        println!("    timeout: {}ms", hook.timeout.as_millis());
        println!(
            "    target: {} ({})",
            hook.target.display,
            hook.target.kind.as_str()
        );
        println!("    hook hash: {}", hook.hook_hash);
        println!("    target hash: {}", hook.target.hash);
        println!(
            "    env: BINDPORT_HOOK_EVENTS=<redacted> BINDPORT_HOOK_SOURCES=<redacted> BINDPORT_HOOK_CONTEXT=<redacted>"
        );
    }
}

pub(crate) fn hook_trust_status_display(status: HookTrustStatus) -> String {
    match status {
        HookTrustStatus::Approved { scope } => format!("approved ({})", scope.as_str()),
        HookTrustStatus::Denied { scope } => format!("denied ({})", scope.as_str()),
        HookTrustStatus::Changed => String::from("changed"),
        HookTrustStatus::Pending => String::from("pending"),
    }
}

pub(crate) fn hook_events_display(events: &[HookEvent]) -> String {
    events
        .iter()
        .map(|event| event.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
