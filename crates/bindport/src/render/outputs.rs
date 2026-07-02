use super::*;

pub(crate) fn configured_outputs(
    config: &ResolvedConfig,
) -> Result<Vec<EffectiveOutputConfig>, OutputConfigError> {
    config
        .loaded
        .as_ref()
        .map(|loaded| loaded.config.effective_outputs())
        .transpose()
        .map(|outputs| outputs.unwrap_or_default())
}

pub(crate) fn selected_outputs(
    outputs: Vec<EffectiveOutputConfig>,
    output_name: Option<&str>,
) -> Result<Vec<EffectiveOutputConfig>, RenderCommandError> {
    let Some(name) = output_name else {
        return Ok(outputs);
    };

    let selected = outputs
        .into_iter()
        .filter(|output| output.name == name)
        .collect::<Vec<_>>();

    if selected.is_empty() {
        return Err(RenderCommandError::InvalidArgument(format!(
            "output `{name}` is not configured or is disabled"
        )));
    }

    Ok(selected)
}

pub(crate) fn render_outputs_for_events(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    invocation: RenderInvocation<'_>,
) -> Result<usize, RenderCommandError> {
    if invocation.events.is_empty() {
        return Ok(0);
    }

    let mut events = invocation.events.clone();
    collect_stale_reconcile_event(registry, &mut events)?;

    let rendered = render_outputs(
        cwd,
        config,
        registry,
        invocation.outputs,
        invocation.dry_run,
        invocation.mode,
        invocation.report,
    )?;
    let mode = if invocation.dry_run {
        HookRunMode::DryRun
    } else {
        HookRunMode::Run
    };
    run_hooks_for_events(cwd, config, &events, rendered > 0, mode);

    Ok(rendered)
}

pub(crate) fn render_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    outputs: Vec<EffectiveOutputConfig>,
    dry_run: bool,
    mode: RenderMode,
    report: RenderReport,
) -> Result<usize, RenderCommandError> {
    let resolver = TemplateResolver::new(
        Some(project_template_dir(cwd, config)),
        global_template_dir(),
    );
    let snapshot = registry.status_snapshot()?;
    let routes = route_records(snapshot.services);
    let current_route_keys = routes
        .iter()
        .map(|route| route.key.clone())
        .collect::<BTreeSet<_>>();
    let base_dir = output_base_dir(cwd, config);
    let mut rendered = 0;

    for output in outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let delete_route_keys = delete_route_keys(&output, &routes);
        let render_routes = routes
            .iter()
            .filter(|route| !delete_route_keys.contains(&route.key))
            .cloned()
            .collect::<Vec<_>>();
        let plan = render_output_routes(&render_config, &template.contents, &render_routes)?;

        if dry_run {
            if report == RenderReport::Print {
                println!("would render {}: {} files", output.name, plan.files.len());
                for file in &plan.files {
                    println!("  {}", file.target);
                }
            }
            rendered += plan.files.len();
            continue;
        }

        let ownership = registry.output_file_ownership(&output.name)?;
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        let removed = remove_output_files_for_lifecycle(
            registry,
            &output,
            &ownership,
            &current_route_keys,
            &delete_route_keys,
            &base_dir,
            &render_config,
        )?;
        let write_summary = match mode {
            RenderMode::Normal => {
                let written = write_render_plan(&plan, &base_dir, &write_ownership)?;
                record_written_output_files(registry, &output, &written)?;
                RenderWriteSummary {
                    written: written.len(),
                    external_modified: 0,
                }
            }
            RenderMode::Repair => {
                write_repair_render_plan(registry, &output, &plan, &base_dir, &write_ownership)?
            }
        };

        if report == RenderReport::Print {
            let verb = if mode == RenderMode::Repair {
                "repaired"
            } else {
                "rendered"
            };
            println!("{verb} {}: {} files", output.name, write_summary.written);
            if removed > 0 {
                println!("removed {}: {} files", output.name, removed);
            }
            if write_summary.external_modified > 0 {
                println!(
                    "preserved {}: {} externally modified files",
                    output.name, write_summary.external_modified
                );
            }
        }
        rendered += write_summary.written;
    }

    Ok(rendered)
}

pub(crate) struct RenderWriteSummary {
    pub(crate) written: usize,
    pub(crate) external_modified: usize,
}

pub(crate) fn write_repair_render_plan(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[AdapterOutputFileOwnership],
) -> Result<RenderWriteSummary, RenderCommandError> {
    let mut summary = RenderWriteSummary {
        written: 0,
        external_modified: 0,
    };

    for file in &plan.files {
        let single_file_plan = RenderPlan {
            output: plan.output.clone(),
            files: vec![file.clone()],
        };
        match write_render_plan(&single_file_plan, base_dir, ownership) {
            Ok(written) => {
                record_written_output_files(registry, output, &written)?;
                summary.written += written.len();
            }
            Err(OutputFileError::ExternalModified { path }) => {
                let expected_hash = ownership
                    .iter()
                    .find(|owned| owned.path == path)
                    .map(|owned| owned.content_hash.clone());
                registry.record_output_file(&OutputFileRecord {
                    output_name: output.name.clone(),
                    route_key: file.route_key.clone(),
                    rendered_path: path,
                    status: OutputFileStatus::Error,
                    reason: Some(String::from("external_modified")),
                    content_hash: expected_hash,
                    template_hash: None,
                    lease_id: None,
                    run_id: None,
                })?;
                summary.external_modified += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(summary)
}

pub(crate) fn record_written_output_files(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    written: &[bindport_adapters::WrittenOutputFile],
) -> Result<(), RegistryError> {
    for file in written {
        registry.record_output_file(&OutputFileRecord {
            output_name: output.name.clone(),
            route_key: file.route_key.clone(),
            rendered_path: file.path.clone(),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(file.content_hash.clone()),
            template_hash: None,
            lease_id: None,
            run_id: None,
        })?;
    }

    Ok(())
}
