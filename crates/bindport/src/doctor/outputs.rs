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
        if !print_doctor_output(output, &resolver, &route_snapshot, &base_dir) {
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
) -> bool {
    println!("output {}:", output.name);
    println!("  target: {}", output.target);
    println!(
        "  root: {}",
        output.root.as_deref().unwrap_or("<derived from target>")
    );
    println!("  auto-render: {}", output.auto_render);

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

    let render_config = OutputRenderConfig::from(output);
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

    true
}
