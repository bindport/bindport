use super::*;

pub(crate) fn select_open_service<'a>(
    services: &'a [StatusService],
    options: &OpenOptions,
) -> Result<&'a StatusService, OpenCommandError> {
    let matches = services
        .iter()
        .filter(|service| service.state == "active")
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
            format!("no active BindPort service matched `{project}/{service}`")
        }
        (None, Some(service)) => format!("no active BindPort service matched `{service}`"),
        (Some(project), None) => format!("no active BindPort service matched project `{project}`"),
        (None, None) => String::from("no active BindPort services recorded"),
    }
}

pub(crate) fn open_ambiguous_message(options: &OpenOptions, services: &[&StatusService]) -> String {
    let matches = services
        .iter()
        .map(|service| format!("{}/{}", service.project, service.service))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");

    match &options.service {
        Some(service) => {
            format!(
                "multiple active services matched `{service}`; pass --project. matches: {matches}"
            )
        }
        None => {
            format!("multiple active services recorded; pass a service name. matches: {matches}")
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
