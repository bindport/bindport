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
