// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn service_paths_infer_service_from_cwd() {
    let root = temp_test_dir("service-paths");
    let web_src = root.join("apps").join("web").join("src");
    let api = root.join("apps").join("api");
    fs::create_dir_all(&web_src).expect("web src");
    fs::create_dir_all(&api).expect("api dir");
    let config = parse_config(
        ConfigFormat::Toml,
        "project = \"demo\"\n[[services]]\nname = \"web\"\npath = \"apps/web\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
    )
    .expect("config");

    assert_eq!(
        config.configured_service_name_for_cwd(&root, &web_src),
        Some("web")
    );
    let matched = config
        .configured_service_for_cwd(&root, &web_src)
        .expect("matched web service");
    assert_eq!(matched.name, "web");
    assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
    assert_eq!(
        config.configured_service_name_for_cwd(&root, &api),
        Some("api")
    );
    assert_eq!(config.configured_service_name_for_cwd(&root, &root), None);
}

#[test]
fn deepest_service_path_match_wins() {
    let root = temp_test_dir("service-path-depth");
    let api_src = root.join("apps").join("api").join("src");
    fs::create_dir_all(&api_src).expect("api src");
    let config = parse_config(
        ConfigFormat::Toml,
        "project = \"demo\"\n[[services]]\nname = \"apps\"\npath = \"apps\"\n[[services]]\nname = \"api\"\npath = \"apps/api\"\n",
    )
    .expect("config");

    assert_eq!(
        config.configured_service_name_for_cwd(&root, &api_src),
        Some("api")
    );
    let matched = config
        .configured_service_for_cwd(&root, &api_src)
        .expect("matched api service");
    assert_eq!(matched.name, "api");
    assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);
}

#[test]
fn configured_service_precedence_covers_path_ties_and_single_service() {
    let root = temp_test_dir("service-precedence");
    let web_src = root.join("apps").join("web").join("src");
    fs::create_dir_all(&web_src).expect("web src");
    let config = BindPortConfig {
        services: Some(vec![
            ServiceConfig {
                path: Some(String::from("apps/web")),
                ..ServiceConfig::default()
            },
            ServiceConfig {
                name: Some(String::from("empty-path")),
                path: Some(String::from(" ")),
                ..ServiceConfig::default()
            },
            ServiceConfig {
                name: Some(String::from("first-web")),
                path: Some(String::from("apps/web")),
                ..ServiceConfig::default()
            },
            ServiceConfig {
                name: Some(String::from("second-web")),
                path: Some(String::from("apps/web")),
                ..ServiceConfig::default()
            },
            ServiceConfig {
                name: Some(String::from("apps")),
                path: Some(String::from("apps")),
                ..ServiceConfig::default()
            },
        ]),
        ..BindPortConfig::default()
    };

    let matched = config
        .configured_service_for_cwd(&root, &web_src)
        .expect("matched first web service");
    assert_eq!(matched.name, "first-web");
    assert_eq!(matched.source, ConfiguredServiceSource::PathMatch);

    let explicit = BindPortConfig {
        service: Some(String::from("explicit")),
        services: config.services.clone(),
        ..BindPortConfig::default()
    };
    assert_eq!(
        explicit.configured_service_for_cwd(&root, &web_src),
        Some(ConfiguredService {
            name: "explicit",
            source: ConfiguredServiceSource::ServiceField
        })
    );

    let single = BindPortConfig {
        services: Some(vec![ServiceConfig {
            name: Some(String::from("solo")),
            ..ServiceConfig::default()
        }]),
        ..BindPortConfig::default()
    };
    assert_eq!(
        single.configured_service_for_cwd(&root, &root),
        Some(ConfiguredService {
            name: "solo",
            source: ConfiguredServiceSource::SingleService
        })
    );
}
