use super::*;

pub(crate) fn select_open_service<'a>(
    services: &'a [StatusService],
    options: &OpenOptions,
) -> Result<&'a StatusService, OpenCommandError> {
    let matches = services
        .iter()
        .filter(|service| matches!(service.state.as_str(), "active" | "reserved"))
        .filter(|service| {
            options
                .service
                .as_ref()
                .is_none_or(|wanted| service.service == *wanted)
        })
        .filter(|service| {
            options
                .project
                .as_ref()
                .is_none_or(|wanted| service.project == *wanted)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [service] => Ok(service),
        [] => Err(OpenCommandError::Selection(open_not_found_message(options))),
        _ => Err(OpenCommandError::Selection(open_ambiguous_message(
            options, &matches,
        ))),
    }
}

pub(crate) fn open_not_found_message(options: &OpenOptions) -> String {
    match (&options.project, &options.service) {
        (Some(project), Some(service)) => {
            format!("no active or reserved BindPort service matched `{project}/{service}`")
        }
        (None, Some(service)) => {
            format!("no active or reserved BindPort service matched `{service}`")
        }
        (Some(project), None) => {
            format!("no active or reserved BindPort service matched project `{project}`")
        }
        (None, None) => String::from("no active or reserved BindPort services recorded"),
    }
}

pub(crate) fn open_ambiguous_message(options: &OpenOptions, services: &[&StatusService]) -> String {
    let matches = services
        .iter()
        .map(|service| {
            let scope = service
                .worktree_path
                .as_deref()
                .map(|path| format!("worktree {path}"))
                .or_else(|| {
                    service
                        .identity_key
                        .as_deref()
                        .map(|identity| format!("identity {identity}"))
                })
                .unwrap_or_else(|| String::from("unscoped"));
            format!("{}/{} ({scope})", service.project, service.service)
        })
        .collect::<Vec<_>>()
        .join(", ");

    match &options.service {
        Some(service) => {
            let project_count = services
                .iter()
                .map(|service| service.project.as_str())
                .collect::<BTreeSet<_>>()
                .len();
            if options.project.is_none() && project_count > 1 {
                format!(
                    "multiple active or reserved services matched `{service}`; pass --project or omit --registry-wide to select the current worktree. matches: {matches}"
                )
            } else {
                format!(
                    "multiple active or reserved services matched `{service}`; omit --registry-wide to select the current worktree. matches: {matches}"
                )
            }
        }
        None => {
            format!(
                "multiple active or reserved services recorded; pass a service name to select the current worktree. matches: {matches}"
            )
        }
    }
}

pub(crate) fn best_service_url(service: &StatusService) -> String {
    service
        .route_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(&service.url)
        .to_string()
}

pub(crate) fn best_registry_service_url(service: &RegistryService) -> String {
    service
        .route_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("http://{}:{}", service.host, service.port))
}
