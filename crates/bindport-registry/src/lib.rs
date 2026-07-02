// SPDX-License-Identifier: MIT

use std::{
    env, fmt, fs,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use bindport_core::{SERVICE_NAME, ServiceIdentity};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

mod cleanup;
mod clock;
mod connection;
mod constants;
mod error;
mod health;
mod lease;
mod outputs;
mod process;
mod schema;
mod status;

pub use cleanup::*;
pub(crate) use clock::*;
pub use connection::*;
pub use constants::*;
pub use error::*;
pub(crate) use health::*;
pub use lease::*;
pub use outputs::*;
pub(crate) use process::*;
pub use status::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        net::{Shutdown, TcpListener},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_run_start(project: &str, service: &str, port: u16, pid: u32) -> RunStart {
        RunStart {
            project: String::from(project),
            service: String::from(service),
            identity: None,
            host: String::from("127.0.0.1"),
            port,
            hostname: None,
            route_url: None,
            health_url: None,
            pid,
            command: String::from("next dev"),
            cwd: PathBuf::from("/tmp/bindport"),
        }
    }

    fn mark_latest_run_started_before_grace(registry: &Registry) {
        registry
            .connection
            .execute(
                "UPDATE runs
                 SET started_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-5 seconds')",
                [],
            )
            .expect("backdate run start");
    }

    fn free_loopback_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
        listener.local_addr().expect("local addr").port()
    }

    fn start_health_server(status: &'static str) -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind health server");
        let port = listener.local_addr().expect("health server addr").port();

        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut request = Vec::new();
            let mut buffer = [0_u8; 128];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(bytes) => {
                        request.extend_from_slice(&buffer[..bytes]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        break;
                    }
                    Err(_) => return,
                }
            }
            let _ = write!(
                &mut stream,
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Write);
        });

        port
    }

    #[test]
    fn registry_defaults_are_named_for_bindport() {
        assert_eq!(default_registry_directory_name(), "bindport");
        assert_eq!(DEFAULT_REGISTRY_FILE, "registry.sqlite");
    }

    #[cfg(unix)]
    #[test]
    fn registry_creates_private_state_dir_and_database() {
        let path = env::temp_dir()
            .join(format!("bindport-private-registry-{}", std::process::id()))
            .join(DEFAULT_REGISTRY_FILE);
        let parent = path.parent().expect("registry parent");
        let _ = fs::remove_dir_all(parent);

        let _registry = Registry::open(&path).expect("registry");

        let dir_mode = fs::metadata(parent)
            .expect("parent metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&path)
            .expect("registry metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn registry_records_finished_runs_for_status() {
        let mut registry = Registry::open(temp_registry_path("finished")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.schema_version, STATUS_SCHEMA_VERSION);
        assert!(snapshot.outputs.is_empty());
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.services[0].state, "stopped");
        assert_eq!(snapshot.services[0].port, 29_123);
        assert_eq!(snapshot.services[0].url, "http://127.0.0.1:29123");
        assert_eq!(snapshot.services[0].hostname.as_deref(), None);
        assert_eq!(snapshot.services[0].route_url.as_deref(), None);
        assert!(snapshot.services[0].outputs.is_empty());
        assert!(snapshot.services[0].proxy.is_none());
        assert_eq!(snapshot.services[0].exit_code, Some(0));
        assert_eq!(snapshot.runs.len(), 1);
    }

    #[test]
    fn health_targets_are_restricted_to_loopback_without_dns() {
        let target = http_health_target("http://127.0.0.1:29100/health")
            .expect("loopback URL parses")
            .expect("loopback URL is supported");
        assert_eq!(
            target.address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_100)
        );
        assert_eq!(target.authority, "127.0.0.1:29100");
        assert_eq!(target.path, "/health");

        let target = http_health_target("http://feature.branch.localhost:29101/ready")
            .expect("localhost URL parses")
            .expect("localhost URL is supported");
        assert_eq!(
            target.address,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 29_101)
        );
        assert_eq!(target.authority, "feature.branch.localhost:29101");

        assert!(
            http_health_target("http://169.254.169.254/latest")
                .expect("metadata URL parses")
                .is_none()
        );
        assert!(
            http_health_target("http://example.invalid/health")
                .expect("external URL parses")
                .is_none()
        );
        assert!(
            http_health_target("https://127.0.0.1:29100/health")
                .expect("https URL parses as unsupported")
                .is_none()
        );
    }

    #[test]
    fn health_targets_reject_request_line_injection_bytes() {
        for url in [
            "http://127.0.0.1:6379/\r\nFLUSHALL\r\n",
            "http://127.0.0.1:6379/path with-space",
            "http://local\nhost:6379/health",
        ] {
            assert!(http_health_target(url).is_err(), "{url:?}");
        }
    }

    #[test]
    fn status_health_is_pending_during_startup_grace() {
        let mut registry = Registry::open(temp_registry_path("health-pending")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

        registry.record_run_started(&run).expect("record start");

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "pending");
    }

    #[test]
    fn status_health_reports_healthy_http_response() {
        let direct_port = start_health_server("204 No Content");
        let direct_url = format!("http://127.0.0.1:{direct_port}/health");
        let direct_target = http_health_target(&direct_url)
            .expect("direct health URL parses")
            .expect("direct health URL is supported");
        let direct_status = probe_http_target(&direct_target).expect("direct health probe");
        assert!((200..400).contains(&direct_status));

        let health_port = start_health_server("204 No Content");
        let mut registry = Registry::open(temp_registry_path("health-healthy")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{health_port}/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "healthy");
    }

    #[test]
    fn status_health_reports_failing_http_response() {
        let mut registry = Registry::open(temp_registry_path("health-failing")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(format!("http://127.0.0.1:{}/health", free_loopback_port()));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "failing");
    }

    #[test]
    fn status_health_reports_unknown_for_non_loopback_http_targets() {
        let mut registry =
            Registry::open(temp_registry_path("health-non-loopback")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(String::from("http://192.0.2.1/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "unknown");
    }

    #[test]
    fn status_health_reports_unknown_for_unsupported_schemes() {
        let mut registry =
            Registry::open(temp_registry_path("health-unsupported")).expect("registry");
        let mut run = test_run_start("bindport", "web", 29_123, std::process::id());
        run.health_url = Some(String::from("https://web.localhost/health"));

        registry.record_run_started(&run).expect("record start");
        mark_latest_run_started_before_grace(&registry);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(
            snapshot.services[0].health_url.as_deref(),
            run.health_url.as_deref()
        );
        assert_eq!(snapshot.services[0].health, "unknown");
    }

    #[test]
    fn clean_leases_dry_run_counts_without_deleting_stopped_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-dry-run")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let summary = registry
            .clean_leases(&[CleanState::Stopped], true)
            .expect("clean dry-run");

        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.stale_leases, 0);
        assert_eq!(summary.runs, 1);
        assert_eq!(summary.total_leases(), 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services.len(), 1);
        assert_eq!(snapshot.runs.len(), 1);
    }

    #[test]
    fn clean_leases_removes_stopped_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-stopped")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start("bindport", "next", 29_123, 12_345))
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let summary = registry
            .clean_leases(&[CleanState::Stopped, CleanState::Stale], false)
            .expect("clean");

        assert_eq!(summary.stopped_leases, 1);
        assert_eq!(summary.stale_leases, 0);
        assert_eq!(summary.runs, 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    }

    #[test]
    fn output_file_ownership_returns_rendered_files_with_hashes() {
        let mut registry = Registry::open(temp_registry_path("output-files")).expect("registry");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-1"),
                rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record rendered file");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-2"),
                rendered_path: PathBuf::from("/tmp/bindport/route-2.yml"),
                status: OutputFileStatus::Error,
                reason: Some(String::from("template_error")),
                content_hash: None,
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record error file");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(
            ownership,
            vec![OutputFileOwnership {
                route_key: String::from("route-1"),
                path: PathBuf::from("/tmp/bindport/route-1.yml"),
                content_hash: String::from("hash-1")
            }]
        );

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.outputs.len(), 1);
        assert_eq!(snapshot.outputs[0].name, "traefik");
        assert_eq!(snapshot.outputs[0].rendered, 1);
        assert_eq!(snapshot.outputs[0].error, 1);
    }

    #[test]
    fn output_file_ownership_keeps_external_modified_expected_hashes() {
        let mut registry =
            Registry::open(temp_registry_path("output-files-modified")).expect("registry");
        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: String::from("route-1"),
                rendered_path: PathBuf::from("/tmp/bindport/route-1.yml"),
                status: OutputFileStatus::Error,
                reason: Some(String::from("external_modified")),
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: None,
                run_id: None,
            })
            .expect("record external modification");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(
            ownership,
            vec![OutputFileOwnership {
                route_key: String::from("route-1"),
                path: PathBuf::from("/tmp/bindport/route-1.yml"),
                content_hash: String::from("hash-1")
            }]
        );
    }

    #[test]
    fn record_output_file_upserts_by_output_and_route() {
        let mut registry =
            Registry::open(temp_registry_path("output-files-upsert")).expect("registry");
        let mut record = OutputFileRecord {
            output_name: String::from("traefik"),
            route_key: String::from("route-1"),
            rendered_path: PathBuf::from("/tmp/bindport/old.yml"),
            status: OutputFileStatus::Rendered,
            reason: None,
            content_hash: Some(String::from("old-hash")),
            template_hash: Some(String::from("template-1")),
            lease_id: Some(1),
            run_id: Some(2),
        };

        registry
            .record_output_file(&record)
            .expect("record first file");
        record.rendered_path = PathBuf::from("/tmp/bindport/new.yml");
        record.content_hash = Some(String::from("new-hash"));
        registry
            .record_output_file(&record)
            .expect("record updated file");

        let ownership = registry
            .output_file_ownership("traefik")
            .expect("ownership records");

        assert_eq!(ownership.len(), 1);
        assert_eq!(ownership[0].path, PathBuf::from("/tmp/bindport/new.yml"));
        assert_eq!(ownership[0].content_hash, "new-hash");
    }

    #[test]
    fn auto_render_reservations_apply_debounce_windows() {
        let mut registry =
            Registry::open(temp_registry_path("auto-render-reservation")).expect("registry");

        let first = registry
            .reserve_auto_render_at("traefik", 250, 1_000)
            .expect("first reservation");
        let second = registry
            .reserve_auto_render_at("traefik", 250, 1_100)
            .expect("debounced reservation");
        let disabled = registry
            .reserve_auto_render_at("traefik", 0, 1_100)
            .expect("disabled debounce");

        assert_eq!(first, Duration::from_millis(0));
        assert_eq!(second, Duration::from_millis(150));
        assert_eq!(disabled, Duration::from_millis(0));
    }

    #[cfg(unix)]
    #[test]
    fn clean_leases_reconciles_and_removes_stale_runs() {
        let mut registry = Registry::open(temp_registry_path("clean-stale")).expect("registry");
        registry
            .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
            .expect("record start");

        let summary = registry
            .clean_leases(&[CleanState::Stale], false)
            .expect("clean stale");

        assert_eq!(summary.stopped_leases, 0);
        assert_eq!(summary.stale_leases, 1);
        assert_eq!(summary.runs, 1);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert!(snapshot.services.is_empty());
        assert!(snapshot.runs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn prune_oldest_stale_leases_removes_only_pressure_excess() {
        let mut registry = Registry::open(temp_registry_path("prune-stale")).expect("registry");

        for index in 0..4 {
            registry
                .record_run_started(&test_run_start(
                    "bindport",
                    &format!("web-{index}"),
                    29_500 + index,
                    2_000_000_000 + index as u32,
                ))
                .expect("record stale candidate");
        }

        let dry_run = registry
            .prune_oldest_stale_leases(29_500, 29_503, 2, true)
            .expect("dry-run prune stale");

        assert_eq!(dry_run.stale_leases, 2);
        assert_eq!(dry_run.runs, 2);
        assert_eq!(
            registry.status_snapshot().expect("snapshot").services.len(),
            4
        );

        let summary = registry
            .prune_oldest_stale_leases(29_500, 29_503, 2, false)
            .expect("prune stale");

        assert_eq!(summary.stale_leases, 2);
        assert_eq!(summary.runs, 2);

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services.len(), 2);
        assert_eq!(snapshot.runs.len(), 2);
    }

    #[test]
    fn registry_records_identity_fields_for_status() {
        let mut registry = Registry::open(temp_registry_path("identity")).expect("registry");
        let identity = ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: Some(bindport_core::GitIdentity {
                worktree_path: PathBuf::from("/tmp/bindport-worktree"),
                worktree_hash: String::from("abc123"),
                git_common_dir: PathBuf::from("/tmp/bindport-worktree/.git"),
                branch: String::from("feature/tree"),
                branch_label: String::from("feature-tree"),
                commit: String::from("1234567"),
            }),
            identity_key: String::from("v1:identity"),
        };
        let started = registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: Some(String::from("feature-tree.bindport.localhost")),
                route_url: Some(String::from("http://feature-tree.bindport.localhost")),
                health_url: None,
                pid: 12_346,
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport-worktree"),
            })
            .expect("record start");

        registry
            .record_run_finished(started, Some(0))
            .expect("record finish");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(
            service.worktree_path.as_deref(),
            Some("/tmp/bindport-worktree")
        );
        assert_eq!(service.worktree_hash.as_deref(), Some("abc123"));
        assert_eq!(service.branch.as_deref(), Some("feature/tree"));
        assert_eq!(service.branch_label.as_deref(), Some("feature-tree"));
        assert_eq!(service.commit.as_deref(), Some("1234567"));
        assert_eq!(service.identity_key.as_deref(), Some("v1:identity"));
        assert_eq!(
            service.hostname.as_deref(),
            Some("feature-tree.bindport.localhost")
        );
        assert_eq!(
            service.route_url.as_deref(),
            Some("http://feature-tree.bindport.localhost")
        );
    }

    #[test]
    fn status_snapshot_reports_service_outputs_and_traefik_proxy_alias() {
        let mut registry =
            Registry::open(temp_registry_path("status-service-outputs")).expect("registry");
        let identity = test_identity("v1:status-output");
        let started = registry
            .record_run_started(&RunStart {
                project: identity.project.clone(),
                service: identity.service.clone(),
                identity: Some(identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: Some(String::from("status.localhost")),
                route_url: Some(String::from("http://status.localhost")),
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record start");

        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key: identity.identity_key,
                rendered_path: PathBuf::from("/tmp/bindport/traefik/web.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: Some(started.lease_id),
                run_id: Some(started.run_id),
            })
            .expect("record rendered file");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(snapshot.outputs.len(), 1);
        assert_eq!(snapshot.outputs[0].name, "traefik");
        assert_eq!(snapshot.outputs[0].rendered, 1);
        assert_eq!(service.outputs.len(), 1);
        assert_eq!(service.outputs[0].name, "traefik");
        assert_eq!(service.outputs[0].status, "rendered");
        assert_eq!(service.outputs[0].reason, None);
        assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/web.yml");
        let proxy = service.proxy.as_ref().expect("traefik proxy alias");
        assert_eq!(proxy.adapter, "traefik");
        assert!(proxy.rendered);
        assert_eq!(
            proxy.target.as_deref(),
            Some("/tmp/bindport/traefik/web.yml")
        );
    }

    #[test]
    fn status_snapshot_links_outputs_for_services_without_identity_keys() {
        let mut registry =
            Registry::open(temp_registry_path("status-output-fallback-key")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start(
                "bindport",
                "next",
                29_123,
                std::process::id(),
            ))
            .expect("record start");
        let route_key = {
            let snapshot = registry.status_snapshot().expect("snapshot");
            let service = &snapshot.services[0];

            assert!(service.identity_key.is_none());
            format!(
                "{}:{}:{}:{}:{}",
                service.project, service.service, service.host, service.port, service.started_at
            )
        };

        registry
            .record_output_file(&OutputFileRecord {
                output_name: String::from("traefik"),
                route_key,
                rendered_path: PathBuf::from("/tmp/bindport/traefik/next.yml"),
                status: OutputFileStatus::Rendered,
                reason: None,
                content_hash: Some(String::from("hash-1")),
                template_hash: Some(String::from("template-1")),
                lease_id: Some(started.lease_id),
                run_id: Some(started.run_id),
            })
            .expect("record rendered file");

        let snapshot = registry.status_snapshot().expect("snapshot");
        let service = &snapshot.services[0];

        assert_eq!(service.outputs.len(), 1);
        assert_eq!(service.outputs[0].name, "traefik");
        assert_eq!(service.outputs[0].path, "/tmp/bindport/traefik/next.yml");
        assert!(service.proxy.as_ref().expect("proxy").rendered);
    }

    #[test]
    fn previous_identity_port_returns_latest_matching_lease() {
        let mut registry = Registry::open(temp_registry_path("previous-port")).expect("registry");
        let first_identity = test_identity("v1:first");
        let second_identity = test_identity("v1:second");
        let first = registry
            .record_run_started(&RunStart {
                project: first_identity.project.clone(),
                service: first_identity.service.clone(),
                identity: Some(first_identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_123,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record first start");
        registry
            .record_run_finished(first, Some(0))
            .expect("record first finish");
        let second = registry
            .record_run_started(&RunStart {
                project: first_identity.project.clone(),
                service: first_identity.service.clone(),
                identity: Some(first_identity.clone()),
                host: String::from("127.0.0.1"),
                port: 29_124,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record second start");
        registry
            .record_run_finished(second, Some(0))
            .expect("record second finish");
        let other = registry
            .record_run_started(&RunStart {
                project: second_identity.project.clone(),
                service: second_identity.service.clone(),
                identity: Some(second_identity),
                host: String::from("127.0.0.1"),
                port: 29_125,
                hostname: None,
                route_url: None,
                health_url: None,
                pid: std::process::id(),
                command: String::from("next dev"),
                cwd: PathBuf::from("/tmp/bindport"),
            })
            .expect("record other start");
        registry
            .record_run_finished(other, Some(0))
            .expect("record other finish");

        assert_eq!(
            registry
                .previous_identity_port(&first_identity.identity_key)
                .expect("previous port"),
            Some(29_124)
        );
        assert_eq!(
            registry
                .previous_identity_port("v1:missing")
                .expect("missing previous port"),
            None
        );
    }

    #[test]
    fn active_ports_reports_active_and_reserved_leases() {
        let mut registry = Registry::open(temp_registry_path("active")).expect("registry");
        registry
            .record_run_started(&test_run_start(
                "bindport",
                "web",
                29_500,
                std::process::id(),
            ))
            .expect("record start");

        assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
    }

    #[test]
    fn record_run_started_rejects_duplicate_active_port() {
        let mut registry =
            Registry::open(temp_registry_path("duplicate-active-port")).expect("registry");
        registry
            .record_run_started(&test_run_start(
                "bindport",
                "web",
                29_500,
                std::process::id(),
            ))
            .expect("record first start");

        let error = registry
            .record_run_started(&test_run_start(
                "bindport",
                "api",
                29_500,
                std::process::id(),
            ))
            .expect_err("duplicate active port");

        assert!(matches!(
            error,
            RegistryError::PortConflict { port: 29_500 }
        ));
        assert_eq!(registry.active_ports().expect("ports"), vec![29_500]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn active_ports_marks_reused_pid_stale_when_start_time_changes() {
        let mut registry =
            Registry::open(temp_registry_path("stale-reused-pid")).expect("registry");
        let started = registry
            .record_run_started(&test_run_start(
                "bindport",
                "web",
                29_500,
                std::process::id(),
            ))
            .expect("record start");
        registry
            .connection
            .execute(
                "UPDATE runs SET process_start_time = 0 WHERE id = ?1",
                params![started.run_id],
            )
            .expect("force stale start time");

        assert!(registry.active_ports().expect("ports").is_empty());

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services[0].state, "stale");
        assert!(snapshot.services[0].exited_at.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn active_ports_marks_dead_pid_stale() {
        let mut registry = Registry::open(temp_registry_path("stale")).expect("registry");
        registry
            .record_run_started(&test_run_start("bindport", "web", 29_500, 2_000_000_000))
            .expect("record start");

        assert!(registry.active_ports().expect("ports").is_empty());

        let snapshot = registry.status_snapshot().expect("snapshot");
        assert_eq!(snapshot.services[0].state, "stale");
        assert!(snapshot.services[0].exited_at.is_some());
        assert_eq!(snapshot.services[0].exit_code, None);
    }

    fn temp_registry_path(name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();

        env::temp_dir().join(format!(
            "bindport-registry-{name}-{}-{now}.sqlite",
            std::process::id()
        ))
    }

    fn test_identity(identity_key: &str) -> ServiceIdentity {
        ServiceIdentity {
            project: String::from("bindport"),
            service: String::from("web"),
            git: None,
            identity_key: identity_key.to_owned(),
        }
    }
}
