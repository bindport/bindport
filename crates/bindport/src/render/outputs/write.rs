use super::*;

pub(crate) fn write_normal_render_plan(
    registry: &mut Registry,
    output: &EffectiveOutputConfig,
    scope: &OutputFileScope,
    plan: &RenderPlan,
    base_dir: &Path,
    ownership: &[AdapterOutputFileOwnership],
) -> Result<RenderWriteSummary, RenderCommandError> {
    let mut written = 0;

    for file in &plan.files {
        let single_file_plan = RenderPlan {
            output: plan.output.clone(),
            files: vec![file.clone()],
        };
        let result = write_render_plan(&single_file_plan, base_dir, ownership)?;
        record_written_output_files(registry, output, scope, &result)?;
        written += result.len();
    }

    Ok(RenderWriteSummary {
        written,
        adopted: 0,
        external_modified: 0,
    })
}
