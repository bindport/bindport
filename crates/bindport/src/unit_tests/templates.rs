// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn template_expansion_reports_syntax_errors() {
    let identity = ServiceIdentity {
        project: String::from("demo"),
        service: String::from("web"),
        git: None,
        identity_key: String::from("v1:test"),
    };
    let values = TemplateValues::new(&identity, 29100, None, None, None);

    assert!(matches!(
        expand_template("{project", &values),
        Err(TemplateError::Unclosed { .. })
    ));
    assert_eq!(
        expand_template("{project", &values)
            .expect_err("unclosed")
            .to_string(),
        "unclosed template placeholder in `{project`"
    );
    assert!(matches!(
        expand_template("project}", &values),
        Err(TemplateError::Unopened { .. })
    ));
    assert_eq!(
        expand_template("project}", &values)
            .expect_err("unopened")
            .to_string(),
        "unmatched `}` in template `project}`"
    );
    assert!(matches!(
        expand_template("{missing}", &values),
        Err(TemplateError::UnknownPlaceholder { .. })
    ));
    assert_eq!(
        expand_template("{missing}", &values)
            .expect_err("unknown placeholder")
            .to_string(),
        "unknown or unavailable template placeholder `missing` in `{missing}`"
    );
}

#[test]
fn template_values_include_git_and_fallback_context() {
    let identity = ServiceIdentity {
        project: String::from("demo"),
        service: String::from("web"),
        git: Some(bindport_core::GitIdentity {
            worktree_path: PathBuf::from("/workspace/demo-feature-tree"),
            worktree_hash: String::from("abc123456789"),
            git_common_dir: PathBuf::from("/workspace/demo/.git"),
            branch: String::from("feature/tree"),
            branch_label: String::from("feature-tree"),
            commit: String::from("0123456789abcdef"),
        }),
        identity_key: String::from("v1:test"),
    };
    let values = TemplateValues::new(
        &identity,
        29_100,
        Some("feature-tree.demo.localhost"),
        Some("https://feature-tree.demo.localhost"),
        Some("https://feature-tree.demo.localhost/health"),
    );

    assert_eq!(values.value("port").as_deref(), Some("29100"));
    assert_eq!(values.value("host").as_deref(), Some("127.0.0.1"));
    assert_eq!(
        values.value("url").as_deref(),
        Some("http://127.0.0.1:29100")
    );
    assert_eq!(values.value("project").as_deref(), Some("demo"));
    assert_eq!(values.value("service").as_deref(), Some("web"));
    assert_eq!(
        values.value("hostname").as_deref(),
        Some("feature-tree.demo.localhost")
    );
    assert_eq!(
        values.value("route_url").as_deref(),
        Some("https://feature-tree.demo.localhost")
    );
    assert_eq!(
        values.value("health_url").as_deref(),
        Some("https://feature-tree.demo.localhost/health")
    );
    assert_eq!(values.value("branch").as_deref(), Some("feature-tree"));
    assert_eq!(
        values.value("branch_label").as_deref(),
        Some("feature-tree")
    );
    assert_eq!(values.value("git_branch").as_deref(), Some("feature/tree"));
    assert_eq!(
        values.value("worktree").as_deref(),
        Some("demo-feature-tree")
    );
    assert_eq!(
        values.value("worktree_label").as_deref(),
        Some("demo-feature-tree")
    );
    assert_eq!(
        values.value("worktree_hash").as_deref(),
        Some("abc123456789")
    );
    assert_eq!(values.value("missing"), None);

    let no_git = ServiceIdentity {
        project: String::from("demo"),
        service: String::from("api"),
        git: None,
        identity_key: String::from("v1:no-git"),
    };
    let values = TemplateValues::new(&no_git, 29_101, None, None, None);
    assert_eq!(
        values.value("route_url").as_deref(),
        Some("http://127.0.0.1:29101")
    );
    assert_eq!(values.value("branch").as_deref(), Some("no-branch"));
    assert_eq!(values.value("git_branch").as_deref(), Some("no-branch"));
    assert_eq!(values.value("worktree").as_deref(), Some("demo"));
    assert_eq!(values.value("worktree_hash").as_deref(), Some("no-git"));
}

#[test]
fn sibling_template_values_preserve_direct_and_optional_field_semantics() {
    let identity = ServiceIdentity {
        project: String::from("demo"),
        service: String::from("web"),
        git: None,
        identity_key: String::from("v1:test"),
    };
    let mut siblings = SiblingServices::new();
    siblings.insert(
        String::from("api.v2"),
        RegistryService {
            lease_id: 1,
            project: String::from("demo"),
            service: String::from("api.v2"),
            identity_key: String::from("v1:api"),
            state: String::from("reserved"),
            host: String::from("127.0.0.2"),
            port: 29_200,
            hostname: Some(String::from("api.localhost")),
            route_url: Some(String::from("https://api.localhost")),
            health_url: Some(String::from("https://api.localhost/health")),
        },
    );
    siblings.insert(
        String::from("direct"),
        RegistryService {
            lease_id: 2,
            project: String::from("demo"),
            service: String::from("direct"),
            identity_key: String::from("v1:direct"),
            state: String::from("active"),
            host: String::from("127.0.0.1"),
            port: 29_201,
            hostname: None,
            route_url: None,
            health_url: None,
        },
    );
    let values =
        TemplateValues::new(&identity, 29_100, None, None, None).with_sibling_services(&siblings);

    assert_eq!(
        expand_template(
            "{services.api.v2.port}|{services.api.v2.host}|{services.api.v2.url}|{services.api.v2.hostname}|{services.api.v2.route_url}|{services.api.v2.health_url}",
            &values,
        )
        .expect("sibling fields"),
        "29200|127.0.0.2|http://127.0.0.2:29200|api.localhost|https://api.localhost|https://api.localhost/health"
    );
    assert_eq!(
        expand_template("{services.direct.route_url}", &values).expect("direct route fallback"),
        "http://127.0.0.1:29201"
    );
    assert!(matches!(
        expand_template("{services.direct.hostname}", &values),
        Err(TemplateError::UnavailableSiblingField { .. })
    ));
}

#[test]
fn template_command_parser_validates_sources_and_names() {
    let (command, options) = parse_template_command(&strings([
        "show",
        "--source",
        "built-in",
        "bindport-traefik",
    ]))
    .expect("template command");
    assert_eq!(command, TemplateCommand::Show);
    assert_eq!(options.source, Some(TemplateSource::BuiltIn));
    assert_eq!(options.name.as_deref(), Some("bindport-traefik"));

    let (command, _) = parse_template_command(&strings(["-h"])).expect("template help");
    assert_eq!(command, TemplateCommand::Help);
    assert_eq!(
        parse_template_source("builtin").expect("builtin alias"),
        TemplateSource::BuiltIn
    );
    assert!(parse_template_command(&strings(["list", "extra"])).is_err());
    assert!(parse_template_command(&strings(["show"])).is_err());
    assert!(parse_template_command(&strings(["show", "a", "b"])).is_err());
    assert!(parse_template_command(&strings(["show", "--source"])).is_err());
    assert!(parse_template_command(&strings(["show", "--source", "bad", "name"])).is_err());
    assert!(parse_template_command(&strings(["bad"])).is_err());
    assert!(parse_template_command(&strings(["show", "--bad", "name"])).is_err());
}
