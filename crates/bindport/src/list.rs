use super::*;

const LIST_SCHEMA_VERSION: &str = "0.1";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ListOptions {
    pub(crate) json: bool,
    pub(crate) help: bool,
}

#[derive(Debug)]
pub(crate) enum ListCommandError {
    InvalidArgument(String),
    Registry(RegistryError),
    Serialize(serde_json::Error),
}

impl From<RegistryError> for ListCommandError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<serde_json::Error> for ListCommandError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialize(error)
    }
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct ListSnapshot {
    pub(crate) schema_version: &'static str,
    pub(crate) generated_at: String,
    pub(crate) project_count: usize,
    pub(crate) service_count: usize,
    pub(crate) projects: Vec<ListProject>,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct ListProject {
    pub(crate) project: String,
    pub(crate) service_count: usize,
    pub(crate) active: usize,
    pub(crate) stopped: usize,
    pub(crate) stale: usize,
    pub(crate) reserved: usize,
    pub(crate) other: usize,
    pub(crate) services: Vec<ListService>,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub(crate) struct ListService {
    pub(crate) service: String,
    pub(crate) state: String,
    pub(crate) health: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) url: String,
    pub(crate) route_url: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) branch_label: Option<String>,
    pub(crate) worktree_path: Option<String>,
    pub(crate) pid: Option<u32>,
    pub(crate) started_at: String,
    pub(crate) exited_at: Option<String>,
    pub(crate) identity_key: Option<String>,
}

pub(crate) fn run_list_command(args: &[String]) -> ExitCode {
    match run_list_command_result(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(ListCommandError::InvalidArgument(error)) => {
            eprintln!("bindport: {error}");
            eprintln!("usage: bindport list [--json]");
            ExitCode::FAILURE
        }
        Err(ListCommandError::Registry(error)) => {
            print_registry_error(&error);
            ExitCode::FAILURE
        }
        Err(ListCommandError::Serialize(error)) => {
            eprintln!("bindport: failed to serialize list JSON: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_list_command_result(args: &[String]) -> Result<(), ListCommandError> {
    let options = parse_list_options(args)?;
    if options.help {
        print_list_help();
        return Ok(());
    }

    let snapshot = Registry::open_default()?.status_snapshot()?;
    let list = list_snapshot(&snapshot);
    if options.json {
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        print_list(&list);
    }

    Ok(())
}

pub(crate) fn parse_list_options(args: &[String]) -> Result<ListOptions, ListCommandError> {
    let mut options = ListOptions::default();

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => options.help = true,
            "--json" => options.json = true,
            option if option.starts_with("--") => {
                return Err(ListCommandError::InvalidArgument(format!(
                    "unknown list option `{option}`"
                )));
            }
            value => {
                return Err(ListCommandError::InvalidArgument(format!(
                    "unexpected list argument `{value}`"
                )));
            }
        }
    }

    Ok(options)
}

pub(crate) fn list_snapshot(snapshot: &StatusSnapshot) -> ListSnapshot {
    let mut grouped = BTreeMap::<String, Vec<&StatusService>>::new();
    for service in &snapshot.services {
        grouped
            .entry(service.project.clone())
            .or_default()
            .push(service);
    }

    let projects = grouped
        .into_iter()
        .map(|(project, mut services)| {
            services.sort_by(|left, right| {
                left.service
                    .cmp(&right.service)
                    .then_with(|| left.branch_label.cmp(&right.branch_label))
                    .then_with(|| left.worktree_path.cmp(&right.worktree_path))
                    .then_with(|| left.port.cmp(&right.port))
            });
            let services = services.into_iter().map(list_service).collect::<Vec<_>>();

            ListProject {
                project,
                service_count: services.len(),
                active: count_state(&services, "active"),
                stopped: count_state(&services, "stopped"),
                stale: count_state(&services, "stale"),
                reserved: count_state(&services, "reserved"),
                other: services
                    .iter()
                    .filter(|service| {
                        !matches!(
                            service.state.as_str(),
                            "active" | "stopped" | "stale" | "reserved"
                        )
                    })
                    .count(),
                services,
            }
        })
        .collect::<Vec<_>>();

    ListSnapshot {
        schema_version: LIST_SCHEMA_VERSION,
        generated_at: snapshot.generated_at.clone(),
        project_count: projects.len(),
        service_count: snapshot.services.len(),
        projects,
    }
}

pub(crate) fn print_list(snapshot: &ListSnapshot) {
    if snapshot.projects.is_empty() {
        println!("No BindPort services recorded yet.");
        return;
    }

    for project in &snapshot.projects {
        println!(
            "{} ({} services: {} active, {} stopped, {} stale, {} reserved)",
            project.project,
            project.service_count,
            project.active,
            project.stopped,
            project.stale,
            project.reserved
        );
        for service in &project.services {
            let url = service.route_url.as_deref().unwrap_or(&service.url);
            let branch = service
                .branch_label
                .as_deref()
                .or(service.branch.as_deref())
                .unwrap_or("-");
            let pid = service
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| String::from("-"));
            println!(
                "  {}\t{}\t{}:{}\t{}\tbranch {}\tpid {}",
                service.state, service.service, service.host, service.port, url, branch, pid
            );
        }
    }
}

fn list_service(service: &StatusService) -> ListService {
    ListService {
        service: service.service.clone(),
        state: service.state.clone(),
        health: service.health.clone(),
        host: service.host.clone(),
        port: service.port,
        url: service.url.clone(),
        route_url: service.route_url.clone(),
        hostname: service.hostname.clone(),
        branch: service.branch.clone(),
        branch_label: service.branch_label.clone(),
        worktree_path: service.worktree_path.clone(),
        pid: service.pid,
        started_at: service.started_at.clone(),
        exited_at: service.exited_at.clone(),
        identity_key: service.identity_key.clone(),
    }
}

fn count_state(services: &[ListService], state: &str) -> usize {
    services
        .iter()
        .filter(|service| service.state == state)
        .count()
}
