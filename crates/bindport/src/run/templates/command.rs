use super::*;

pub(crate) fn resolved_child_command(
    explicit_command: &[String],
    metadata: &RunMetadata,
) -> Result<Vec<String>, RunnerError> {
    let command = if explicit_command.is_empty() {
        metadata.command.as_deref().unwrap_or(explicit_command)
    } else {
        explicit_command
    };

    if command
        .first()
        .is_none_or(|program| program.trim().is_empty())
    {
        return Err(RunnerError::NoCommand);
    }

    Ok(command.to_vec())
}
