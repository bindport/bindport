use super::*;

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
    let snapshot = output_route_snapshot(registry.status_snapshot()?);
    let current_route_keys = snapshot
        .routes()
        .iter()
        .map(|route| route.key.clone())
        .collect::<BTreeSet<_>>();
    let base_dir = output_base_dir(cwd, config);
    let mut rendered = 0;

    for output in outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let scope = output_file_scope(&base_dir, &render_config)?;
        let delete_route_keys = delete_route_keys(&output, snapshot.routes());
        let render_snapshot = filtered_output_route_snapshot(&snapshot, &delete_route_keys);
        let plan = render_output_routes(&render_config, &template.contents, &render_snapshot)?;

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

        let ownership = registry.output_file_ownership(&output.name, &scope)?;
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        let removed = remove_output_files_for_lifecycle(
            registry,
            LifecycleRemoval {
                output: &output,
                scope: &scope,
                ownership: &ownership,
                current_route_keys: &current_route_keys,
                delete_route_keys: &delete_route_keys,
                base_dir: &base_dir,
                render_config: &render_config,
            },
        )?;
        let write_summary = match mode {
            RenderMode::Normal => {
                let written = write_render_plan(&plan, &base_dir, &write_ownership)?;
                record_written_output_files(registry, &output, &scope, &written)?;
                RenderWriteSummary {
                    written: written.len(),
                    adopted: 0,
                    external_modified: 0,
                }
            }
            RenderMode::Repair => write_repair_render_plan(
                registry,
                &output,
                &scope,
                &plan,
                &base_dir,
                &write_ownership,
            )?,
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
            if write_summary.adopted > 0 {
                println!("adopted {}: {} files", output.name, write_summary.adopted);
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
