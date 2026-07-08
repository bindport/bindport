use super::*;

pub(crate) fn print_doctor_outputs() -> ExitCode {
    println!("BindPort output doctor");

    let cwd = env::current_dir().unwrap_or_else(|_| Path::new(".").into());
    let config = match resolve_config(&cwd) {
        Ok(config) => {
            print_doctor_output_config(&config);
            config
        }
        Err(error) => {
            println!("config: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    let outputs = match configured_outputs(&config) {
        Ok(outputs) => outputs,
        Err(error) => {
            println!("outputs: invalid ({error})");
            return ExitCode::FAILURE;
        }
    };
    print_doctor_hooks(&cwd, &config);

    if outputs.is_empty() {
        println!("outputs: none configured");
        return ExitCode::SUCCESS;
    }

    let mut registry = match Registry::open_default() {
        Ok(registry) => registry,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let snapshot = match registry.status_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let export_snapshot = match registry.export_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            println!("registry: unavailable ({error})");
            return ExitCode::FAILURE;
        }
    };
    let route_snapshot = output_route_snapshot(snapshot);
    let base_dir = output_base_dir(&cwd, &config);
    let resolver = TemplateResolver::new(
        Some(project_template_dir(&cwd, &config)),
        global_template_dir(),
    );
    let mut ok = true;

    println!("routes: {}", route_snapshot.routes().len());
    println!("base dir: {}", base_dir.display());

    for output in &outputs {
        if !print_doctor_output(
            output,
            &resolver,
            &route_snapshot,
            &base_dir,
            &export_snapshot.output_files,
        ) {
            ok = false;
        }
    }

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
pub(crate) fn print_doctor_output_config(config: &ResolvedConfig) {
    match config.loaded.as_ref() {
        Some(loaded) => {
            println!(
                "config: {} ({} {})",
                loaded.path.display(),
                loaded.source.as_str(),
                loaded.format.as_str()
            );
            print_config_local_override(loaded);
        }
        None => match config.fallback_path.as_ref() {
            Some(path) => println!("config: none (optional fallback: {})", path.display()),
            None => println!("config: none (optional fallback unavailable)"),
        },
    }
}

pub(crate) fn print_doctor_output(
    output: &EffectiveOutputConfig,
    resolver: &TemplateResolver,
    snapshot: &OutputRouteSnapshot,
    base_dir: &Path,
    output_files: &[RegistryExportOutputFile],
) -> bool {
    println!("output {}:", output.name);
    println!("  target: {}", output.target);
    println!(
        "  root: {}",
        output.root.as_deref().unwrap_or("<derived from target>")
    );
    println!("  auto-render: {}", output.auto_render);

    let render_config = OutputRenderConfig::from(output);
    let target_host_ok = print_doctor_target_host(output);
    let output_root_ok = print_doctor_output_root(&render_config, base_dir);
    let mut ok = target_host_ok && output_root_ok;
    match output_file_scope(base_dir, &render_config) {
        Ok(scope) => print_doctor_output_ownership(output, &scope, output_files),
        Err(error) => {
            println!("  ownership: unavailable ({error})");
            ok = false;
        }
    }

    let template = match resolver.resolve(&output.template, None) {
        Ok(template) => {
            println!("  template: {} ({})", output.template, template.source);
            if let Some(path) = template.path.as_ref() {
                println!("  template path: {}", path.display());
            }
            if template.wildcard_matches.len() > 1 {
                println!(
                    "  template warning: multiple wildcard matches; using {}",
                    template
                        .wildcard_matches
                        .first()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| String::from("<unknown>"))
                );
            }
            template
        }
        Err(error) => {
            println!("  template: {} (invalid: {error})", output.template);
            return false;
        }
    };

    let plan = match render_output_plan(&render_config, &template.contents, snapshot) {
        Ok(plan) => plan,
        Err(error) => {
            println!("  plan: invalid ({error})");
            return false;
        }
    };
    let planned_files = match render_plan_paths(&plan, base_dir) {
        Ok(planned_files) => planned_files,
        Err(error) => {
            println!("  paths: invalid ({error})");
            return false;
        }
    };

    println!("  planned files: {}", planned_files.len());
    for file in planned_files.iter().take(5) {
        println!("    {} -> {}", file.route_key, file.path.display());
    }
    if planned_files.len() > 5 {
        println!("    ... {} more", planned_files.len() - 5);
    }

    ok
}

fn print_doctor_target_host(output: &EffectiveOutputConfig) -> bool {
    match target_host_kind(&output.target_host) {
        Ok(kind) => {
            println!("  target host: {} ({kind})", output.target_host);
            println!("  target scheme: {}", output.target_scheme);
            true
        }
        Err(reason) => {
            println!("  target host: {} (invalid: {reason})", output.target_host);
            false
        }
    }
}

pub(crate) fn target_host_kind(host: &str) -> Result<&'static str, &'static str> {
    if host.trim().is_empty() {
        return Err("target_host must not be empty");
    }
    if host.trim() != host || host.chars().any(char::is_whitespace) {
        return Err("target_host must not contain whitespace");
    }
    if host.contains("://") {
        return Err("target_host must be a host name or IP address, not a URL");
    }
    if host
        .chars()
        .any(|character| matches!(character, '/' | '?' | '#'))
    {
        return Err("target_host must not include a path, query, or fragment");
    }

    let (ip_text, bracketed) = if let Some(inner) = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        (inner, true)
    } else if host.starts_with('[') || host.ends_with(']') {
        return Err("IPv6 target_host values must use matching brackets");
    } else if host.contains(':') {
        return Err("target_host must not include a port; BindPort adds the route port");
    } else {
        (host, false)
    };

    if let Ok(address) = ip_text.parse::<std::net::IpAddr>() {
        if address.is_unspecified() {
            return Err("target_host must be connectable, not an unspecified bind address");
        }
        if address.is_loopback() {
            return Ok("loopback");
        }
        return Ok("ip address");
    }
    if bracketed {
        return Err("bracketed target_host values must be IPv6 addresses");
    }

    if matches!(host, "localhost") || host.ends_with(".localhost") {
        Ok("loopback")
    } else if matches!(host, "host.docker.internal") {
        Ok("container host gateway")
    } else {
        Ok("custom host")
    }
}

fn print_doctor_output_root(render_config: &OutputRenderConfig, base_dir: &Path) -> bool {
    match output_root_path(base_dir, &render_config.context) {
        Ok(root) => match fs::metadata(&root) {
            Ok(metadata) if metadata.is_dir() => {
                println!("  resolved root: {} (exists)", root.display());
                true
            }
            Ok(_) => {
                println!(
                    "  resolved root: {} (invalid: not a directory)",
                    root.display()
                );
                false
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                println!(
                    "  resolved root: {} (missing, will be created)",
                    root.display()
                );
                true
            }
            Err(error) => {
                println!("  resolved root: {} (unavailable: {error})", root.display());
                false
            }
        },
        Err(error) => {
            println!("  root: invalid ({error})");
            false
        }
    }
}

fn print_doctor_output_ownership(
    output: &EffectiveOutputConfig,
    scope: &OutputFileScope,
    output_files: &[RegistryExportOutputFile],
) {
    let diagnostics = output_ownership_diagnostics(output, scope, output_files);
    if diagnostics.total == 0 {
        println!("  ownership rows: none");
        return;
    }

    println!(
        "  ownership rows: {} current-scope, {} legacy-adoptable, {} foreign/stale",
        diagnostics.current_scope, diagnostics.legacy_adoptable, diagnostics.foreign_or_stale
    );
    if diagnostics.outside_current_root > 0 {
        println!(
            "  ownership warning: {} rows outside current output root",
            diagnostics.outside_current_root
        );
    }
    if diagnostics.external_modified > 0 {
        println!(
            "  ownership warning: {} externally modified DB-owned files",
            diagnostics.external_modified
        );
    }
    if diagnostics.outside_output_root > 0 {
        println!(
            "  ownership warning: {} rows were previously marked outside_output_root",
            diagnostics.outside_output_root
        );
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct OutputOwnershipDiagnostics {
    total: usize,
    current_scope: usize,
    legacy_adoptable: usize,
    foreign_or_stale: usize,
    outside_current_root: usize,
    external_modified: usize,
    outside_output_root: usize,
}

fn output_ownership_diagnostics(
    output: &EffectiveOutputConfig,
    scope: &OutputFileScope,
    output_files: &[RegistryExportOutputFile],
) -> OutputOwnershipDiagnostics {
    let mut diagnostics = OutputOwnershipDiagnostics::default();
    let output_root = scope.output_root.as_ref();

    for file in output_files
        .iter()
        .filter(|file| file.output_name == output.name)
    {
        diagnostics.total += 1;
        let path = Path::new(&file.rendered_path);
        let in_current_root = output_root.is_some_and(|root| path.starts_with(root));

        if file.output_scope == scope.key {
            diagnostics.current_scope += 1;
        } else if file.output_scope == UNSCOPED_OUTPUT_SCOPE && in_current_root {
            diagnostics.legacy_adoptable += 1;
        } else {
            diagnostics.foreign_or_stale += 1;
        }

        if !in_current_root {
            diagnostics.outside_current_root += 1;
        }
        if matches!(file.reason.as_deref(), Some("external_modified")) {
            diagnostics.external_modified += 1;
        }
        if matches!(file.reason.as_deref(), Some("outside_output_root")) {
            diagnostics.outside_output_root += 1;
        }
    }

    diagnostics
}
