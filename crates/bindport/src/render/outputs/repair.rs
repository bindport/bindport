use super::*;

pub(crate) struct RenderWriteSummary {
    pub(crate) written: usize,
    pub(crate) adopted: usize,
    pub(crate) external_modified: usize,
}

pub(crate) fn write_repair_render_plan(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    scope: &OutputFileScope,
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[AdapterOutputFileOwnership],
) -> Result<RenderWriteSummary, RenderCommandError> {
    let mut summary = RenderWriteSummary {
        written: 0,
        adopted: 0,
        external_modified: 0,
    };

    for file in &plan.files {
        let single_file_plan = RenderPlan {
            output: plan.output.clone(),
            files: vec![file.clone()],
        };
        match write_render_plan(&single_file_plan, base_dir, ownership) {
            Ok(written) => {
                record_written_output_files(registry, output, scope, &written)?;
                summary.written += written.len();
            }
            Err(OutputFileError::ExternalModified { path }) => {
                let expected_hash = ownership
                    .iter()
                    .find(|owned| owned.path == path)
                    .map(|owned| owned.content_hash.clone());
                registry.record_output_file(&OutputFileRecord {
                    output_name: output.name.clone(),
                    scope: scope.clone(),
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
            Err(OutputFileError::UnownedTarget { path }) => {
                let existing_contents =
                    fs::read_to_string(&path).map_err(|source| OutputFileError::Io {
                        path: path.clone(),
                        source,
                    })?;
                if existing_contents != file.contents {
                    return Err(OutputFileError::UnownedTarget { path }.into());
                }

                registry.record_output_file(&OutputFileRecord {
                    output_name: output.name.clone(),
                    scope: scope.clone(),
                    route_key: file.route_key.clone(),
                    rendered_path: path,
                    status: OutputFileStatus::Rendered,
                    reason: None,
                    content_hash: Some(bindport_adapters::rendered_content_hash(&file.contents)),
                    template_hash: None,
                    lease_id: None,
                    run_id: None,
                })?;
                summary.adopted += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(summary)
}

pub(crate) fn record_written_output_files(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    scope: &OutputFileScope,
    written: &[bindport_adapters::WrittenOutputFile],
) -> Result<(), RegistryError> {
    for file in written {
        registry.record_output_file(&OutputFileRecord {
            output_name: output.name.clone(),
            scope: scope.clone(),
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
