// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn traefik_is_first_adapter_name() {
    assert_eq!(AdapterKind::Traefik.as_str(), "traefik");
}

#[test]
fn template_sources_have_stable_display_names() {
    assert_eq!(TemplateSource::Project.to_string(), "project");
    assert_eq!(TemplateSource::Global.to_string(), "global");
    assert_eq!(TemplateSource::BuiltIn.to_string(), "built-in");
}

#[test]
fn rejects_unsafe_template_names() {
    for name in [
        "",
        " ../x",
        "../x",
        "nested/name",
        "nested\\name",
        "safe..ish",
    ] {
        assert!(matches!(
            validate_template_name(name),
            Err(TemplateError::InvalidName(_))
        ));
    }

    validate_template_name("bindport-traefik").expect("safe template name");
}

#[test]
fn resolver_reports_missing_templates_by_source() {
    let resolver = TemplateResolver::new(None, None);

    let global_error = resolver
        .resolve("missing", Some(TemplateSource::Global))
        .expect_err("missing global template");
    assert!(matches!(
        global_error,
        TemplateError::NotFound {
            ref name,
            source: Some(TemplateSource::Global),
        } if name == "missing"
    ));
    assert_eq!(
        global_error.to_string(),
        "template `missing` not found in global templates"
    );

    let any_error = resolver
        .resolve("missing", None)
        .expect_err("missing template");
    assert_eq!(any_error.to_string(), "template `missing` not found");
}

#[test]
fn template_errors_expose_io_and_render_sources() {
    let root = temp_test_dir("template-error-sources");
    let io_error = read_template(
        "broken",
        TemplateSource::Project,
        root.join("missing.j2"),
        Vec::new(),
    )
    .expect_err("missing template file");
    assert!(std::error::Error::source(&io_error).is_some());
    assert!(io_error.to_string().contains("missing.j2"));

    let render_error =
        render_template("{{ missing }}", minijinja::context! {}).expect_err("render error");
    assert!(std::error::Error::source(&render_error).is_some());
    assert!(!render_error.to_string().is_empty());

    let invalid = validate_template_name("../x").expect_err("invalid name");
    assert_eq!(
        invalid.to_string(),
        "invalid template name `../x`; use a safe relative name with no path separators or `..`"
    );
    assert!(std::error::Error::source(&invalid).is_none());
}

#[test]
fn resolver_directory_errors_include_template_source_path() {
    let root = temp_test_dir("resolver-directory-errors");
    let file_path = root.join("not-a-directory");
    fs::write(&file_path, "template").expect("template marker");
    let resolver = TemplateResolver::new(Some(file_path.clone()), None);

    let resolve_error = resolver
        .resolve("app", Some(TemplateSource::Project))
        .expect_err("resolve from file path fails");
    assert!(matches!(resolve_error, TemplateError::Io { ref path, .. } if path == &file_path));
    assert!(std::error::Error::source(&resolve_error).is_some());

    let list_error = resolver
        .list(Some(TemplateSource::Project))
        .expect_err("list from file path fails");
    assert!(matches!(list_error, TemplateError::Io { ref path, .. } if path == &file_path));
}

#[test]
fn resolves_project_before_global_before_builtin() {
    let root = temp_test_dir("resolver-priority");
    let project = root.join("project");
    let global = root.join("global");
    fs::create_dir_all(&project).expect("project dir");
    fs::create_dir_all(&global).expect("global dir");
    fs::write(project.join("bindport-traefik"), "project template").expect("project template");
    fs::write(global.join("bindport-traefik"), "global template").expect("global template");

    let resolver = TemplateResolver::new(Some(project), Some(global));
    let resolved = resolver
        .resolve("bindport-traefik", None)
        .expect("project template wins");

    assert_eq!(resolved.source, TemplateSource::Project);
    assert_eq!(resolved.contents, "project template");
}

#[test]
fn resolves_global_before_builtin() {
    let root = temp_test_dir("resolver-global");
    let global = root.join("global");
    fs::create_dir_all(&global).expect("global dir");
    fs::write(global.join("bindport-traefik.j2"), "global template").expect("global template");

    let resolver = TemplateResolver::new(None, Some(global));
    let resolved = resolver
        .resolve("bindport-traefik", None)
        .expect("global template wins");

    assert_eq!(resolved.source, TemplateSource::Global);
    assert_eq!(resolved.contents, "global template");
}

#[test]
fn lists_templates_with_first_match_precedence_and_source_filters() {
    let root = temp_test_dir("resolver-list");
    let project = root.join("project");
    let global = root.join("global");
    fs::create_dir_all(project.join("nested")).expect("project nested dir");
    fs::create_dir_all(&global).expect("global dir");
    fs::write(project.join("app.yaml.j2"), "app").expect("project app");
    fs::write(project.join("bindport-traefik.j2"), "project traefik").expect("project traefik");
    fs::write(project.join(".hidden.j2"), "hidden").expect("hidden template");
    fs::write(project.join("bad..name.j2"), "bad").expect("bad template");
    fs::write(global.join("bindport-traefik.j2"), "global traefik").expect("global traefik");
    fs::write(global.join("global-only.j2"), "global").expect("global only");

    let resolver = TemplateResolver::new(Some(project.clone()), Some(global.clone()));
    let summaries = resolver.list(None).expect("template summaries");
    let by_name = summaries
        .into_iter()
        .map(|summary| (summary.name.clone(), summary))
        .collect::<BTreeMap<_, _>>();

    assert_eq!(by_name["app"].source, TemplateSource::Project);
    assert_eq!(by_name["app"].path, Some(project.join("app.yaml.j2")));
    assert_eq!(by_name["bindport-traefik"].source, TemplateSource::Project);
    assert_eq!(by_name["global-only"].source, TemplateSource::Global);
    assert_eq!(
        by_name["bindport-env-local"].source,
        TemplateSource::BuiltIn
    );
    assert_eq!(by_name["bad"].source, TemplateSource::Project);
    assert!(!by_name.contains_key(".hidden"));

    let global_summaries = resolver
        .list(Some(TemplateSource::Global))
        .expect("global summaries");
    let global_names = global_summaries
        .into_iter()
        .map(|summary| (summary.name, summary.source))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        global_names,
        BTreeMap::from([
            (String::from("bindport-traefik"), TemplateSource::Global),
            (String::from("global-only"), TemplateSource::Global),
        ])
    );
}

#[test]
fn resolves_wildcard_templates_lexicographically() {
    let root = temp_test_dir("resolver-wildcard");
    fs::create_dir_all(&root).expect("template dir");
    fs::write(root.join("app.yaml.j2"), "yaml").expect("yaml template");
    fs::write(root.join("app.00.yml.j2"), "first").expect("first template");
    fs::write(root.join("app.toml.j2"), "toml").expect("toml template");

    let resolver = TemplateResolver::new(Some(root), None);
    let resolved = resolver
        .resolve("app", None)
        .expect("wildcard template resolves");

    assert_eq!(resolved.contents, "first");
    assert_eq!(resolved.wildcard_matches.len(), 3);
}

#[test]
fn render_template_is_strict_and_unescaped() {
    let rendered = render_template(
        "value={{ value }}",
        minijinja::context! {
            value => "<not escaped>",
        },
    )
    .expect("template renders");

    assert_eq!(rendered, "value=<not escaped>");
    assert!(render_template("{{ missing }}", minijinja::context! {}).is_err());
}

#[test]
fn built_in_traefik_template_renders_active_route() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                key => "demo:web:feature",
                state => "active",
                hostname => "feature.demo.localhost",
                slug => "demo-web-feature",
                target_url => "http://127.0.0.1:29100",
            },
            vars => minijinja::context! {},
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("rule: \"Host(`feature.demo.localhost`)\""));
    assert!(rendered.contains("url: \"http://127.0.0.1:29100\""));
}

#[test]
fn built_in_traefik_template_escapes_yaml_scalars() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-traefik", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                key => "demo:web:feature",
                state => "active",
                hostname => "feature\".demo.localhost",
                slug => "demo-web-feature",
                target_url => "http://127.0.0.1:29100/path\"",
            },
            vars => minijinja::context! {
                entrypoints => ["web\nbad"],
                middlewares => ["auth\"middleware"],
            },
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("rule: \"Host(`feature\\\".demo.localhost`)\""));
    assert!(rendered.contains("- \"web\\nbad\""));
    assert!(rendered.contains("- \"auth\\\"middleware\""));
    assert!(rendered.contains("url: \"http://127.0.0.1:29100/path\\\"\""));
}

#[test]
fn built_in_env_local_template_renders_route_metadata() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-env-local", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                project => "demo",
                service => "web",
                state => "active",
                port => 29100,
                host => "127.0.0.1",
                url => "http://127.0.0.1:29100",
                target_url => "http://127.0.0.1:29100",
                hostname => "feature.demo.localhost",
                route_url => "http://feature.demo.localhost",
            },
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("BINDPORT_PROJECT=\"demo\""));
    assert!(rendered.contains("BINDPORT_SERVICE=\"web\""));
    assert!(rendered.contains("BINDPORT_STATE=\"active\""));
    assert!(rendered.contains("PORT=29100"));
    assert!(rendered.contains("BINDPORT_TARGET_URL=\"http://127.0.0.1:29100\""));
    assert!(rendered.contains("BINDPORT_HOSTNAME=\"feature.demo.localhost\""));
    assert!(rendered.contains("BINDPORT_ROUTE_URL=\"http://feature.demo.localhost\""));
}

#[test]
fn built_in_env_local_template_escapes_newlines() {
    let template = TemplateResolver::new(None, None)
        .resolve("bindport-env-local", None)
        .expect("built-in template");
    let rendered = render_template(
        &template.contents,
        minijinja::context! {
            route => minijinja::context! {
                project => "demo\nNODE_OPTIONS=--require ./evil.js",
                service => "web",
                state => "active",
                port => 29100,
                host => "127.0.0.1",
                url => "http://127.0.0.1:29100",
                target_url => "http://127.0.0.1:29100",
                hostname => "feature.demo.localhost",
                route_url => "http://feature.demo.localhost",
            },
        },
    )
    .expect("built-in template renders");

    assert!(rendered.contains("BINDPORT_PROJECT=\"demo\\nNODE_OPTIONS=--require ./evil.js\""));
    assert!(!rendered.contains("\nNODE_OPTIONS=--require ./evil.js\n"));
}
