use super::*;

pub(crate) fn parse_render_command(
    args: &[String],
) -> Result<(RenderCommand, RenderCommandOptions), RenderCommandError> {
    let mut options = RenderCommandOptions::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" => return Ok((RenderCommand::Help, RenderCommandOptions::default())),
            "--all" => options.all = true,
            "--dry-run" => options.dry_run = true,
            "--diff" => options.diff = true,
            "--repair" => options.repair = true,
            "--verbose" | "-v" => options.verbose = true,
            option if option.starts_with("--") => {
                return Err(RenderCommandError::InvalidArgument(format!(
                    "unknown render option `{option}`"
                )));
            }
            output => {
                if options.output.is_some() {
                    return Err(RenderCommandError::InvalidArgument(String::from(
                        "only one output name can be provided",
                    )));
                }
                options.output = Some(output.to_string());
            }
        }

        index += 1;
    }

    if options.all && options.output.is_some() {
        return Err(RenderCommandError::InvalidArgument(String::from(
            "--all cannot be combined with an output name",
        )));
    }
    let exclusive_modes = [options.dry_run, options.diff, options.repair]
        .into_iter()
        .filter(|enabled| *enabled)
        .count();
    if exclusive_modes > 1 {
        return Err(RenderCommandError::InvalidArgument(String::from(
            "only one of --dry-run, --diff, or --repair can be used",
        )));
    }

    Ok((RenderCommand::Render, options))
}
