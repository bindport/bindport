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

    let log = invocation.log;
    let rendered = render_outputs(
        cwd,
        config,
        registry,
        RenderOutputsRequest {
            outputs: invocation.outputs,
            dry_run: invocation.dry_run,
            mode: invocation.mode,
            report: invocation.report,
            log,
        },
    )?;
    let mode = if invocation.dry_run || invocation.mode == RenderMode::Diff {
        HookRunMode::DryRun
    } else {
        HookRunMode::Run
    };
    run_hooks_for_events(cwd, config, &events, rendered > 0, mode, log);

    Ok(rendered)
}

struct RenderOutputsRequest {
    outputs: Vec<EffectiveOutputConfig>,
    dry_run: bool,
    mode: RenderMode,
    report: RenderReport,
    log: DiagnosticLog,
}

fn render_outputs(
    cwd: &Path,
    config: &ResolvedConfig,
    registry: &mut Registry,
    request: RenderOutputsRequest,
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

    request.log.debug(format_args!(
        "render start mode={} dry_run={} report={} outputs={} routes={} base_dir={}",
        request.mode.as_str(),
        request.dry_run,
        request.report.as_str(),
        request.outputs.len(),
        snapshot.routes().len(),
        base_dir.display()
    ));

    for output in request.outputs {
        let template = resolver.resolve(&output.template, None)?;
        let render_config = OutputRenderConfig::from(&output);
        let scope = output_file_scope(&base_dir, &render_config)?;
        let delete_route_keys = delete_route_keys(&output, snapshot.routes());
        let render_snapshot = filtered_output_route_snapshot(&snapshot, &delete_route_keys);
        let plan = render_output_plan(&render_config, &template.contents, &render_snapshot)?;
        let planned_route_keys = plan
            .files
            .iter()
            .map(|file| file.route_key.clone())
            .collect::<BTreeSet<_>>();
        let scope_root = scope
            .output_root
            .as_deref()
            .map(|root| root.display().to_string())
            .unwrap_or_else(|| String::from("<unscoped>"));

        request.log.debug(format_args!(
            "render output={} template={} source={} root={} scope={} target={} routes={} planned={} delete_candidates={}",
            output.name,
            output.template,
            template.source,
            scope_root,
            scope.key,
            output.target,
            render_snapshot.routes().len(),
            plan.files.len(),
            delete_route_keys.len()
        ));

        if request.dry_run {
            if request.report == RenderReport::Print {
                println!("would render {}: {} files", output.name, plan.files.len());
                for file in &plan.files {
                    println!("  {}", file.target);
                }
            }
            rendered += plan.files.len();
            continue;
        }

        let ownership = registry.output_file_ownership(&output.name, &scope)?;
        request.log.debug(format_args!(
            "render ownership output={} rows={}",
            output.name,
            ownership.len()
        ));
        let write_ownership = ownership
            .iter()
            .map(|owned| AdapterOutputFileOwnership {
                path: owned.path.clone(),
                content_hash: owned.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        let lifecycle_removal = LifecycleRemoval {
            output: &output,
            scope: &scope,
            ownership: &ownership,
            current_route_keys: &current_route_keys,
            planned_route_keys: &planned_route_keys,
            delete_route_keys: &delete_route_keys,
            base_dir: &base_dir,
            render_config: &render_config,
        };

        if request.mode == RenderMode::Diff {
            let removal_candidates = lifecycle_removal_candidates(&lifecycle_removal);
            let removals = diff_removable_output_files(
                &removal_candidates,
                &base_dir,
                &render_config.context,
            )?;
            let diffs = diff_render_plan(&plan, &base_dir, &write_ownership)?;
            let diff_summary = print_render_diff(&output.name, &base_dir, &diffs, &removals);
            request.log.debug(format_args!(
                "render diff output={} changed={} removable={}",
                output.name,
                diff_summary.changed_files(),
                removals.len()
            ));
            rendered += diff_summary.changed_files();
            continue;
        }

        let removed = remove_output_files_for_lifecycle(registry, lifecycle_removal)?;
        if removed > 0 {
            request.log.debug(format_args!(
                "render lifecycle output={} removed={removed}",
                output.name
            ));
        }
        let write_summary = match request.mode {
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
            RenderMode::Diff => unreachable!("diff mode returns before writing"),
        };

        request.log.debug(format_args!(
            "render finish output={} written={} removed={} adopted={} external_modified={}",
            output.name,
            write_summary.written,
            removed,
            write_summary.adopted,
            write_summary.external_modified
        ));

        if request.report == RenderReport::Print {
            let verb = if request.mode == RenderMode::Repair {
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

impl RenderMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Diff => "diff",
            Self::Repair => "repair",
        }
    }
}

impl RenderReport {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Quiet => "quiet",
        }
    }
}
