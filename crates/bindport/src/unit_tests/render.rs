// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn render_command_parser_and_output_selection_validate_combinations() {
    let (command, options) =
        parse_render_command(&strings(["traefik", "--dry-run"])).expect("render command");
    assert_eq!(command, RenderCommand::Render);
    assert_eq!(options.output.as_deref(), Some("traefik"));
    assert!(options.dry_run);

    let (_, options) = parse_render_command(&strings(["--diff"])).expect("render diff");
    assert!(options.diff);

    let (command, _) = parse_render_command(&strings(["--help"])).expect("render help");
    assert_eq!(command, RenderCommand::Help);
    assert!(parse_render_command(&strings(["--all", "traefik"])).is_err());
    assert!(parse_render_command(&strings(["--dry-run", "--repair"])).is_err());
    assert!(parse_render_command(&strings(["--dry-run", "--diff"])).is_err());
    assert!(parse_render_command(&strings(["--diff", "--repair"])).is_err());
    assert!(parse_render_command(&strings(["traefik", "debug"])).is_err());

    let outputs = vec![EffectiveOutputConfig {
        name: String::from("traefik"),
        template: String::from("bindport-traefik"),
        root: None,
        target: String::from("{{ route.slug }}.yml"),
        target_host: String::from("127.0.0.1"),
        target_scheme: String::from("http"),
        auto_render: true,
        delete_on: Vec::new(),
        on_failure: OutputFailurePolicy::Warn,
        debounce_ms: 0,
        vars: BTreeMap::new(),
    }];
    let selected = selected_outputs(outputs.clone(), Some("traefik")).expect("selected");
    assert_eq!(selected.len(), 1);
    assert!(selected_outputs(outputs, Some("missing")).is_err());
}
