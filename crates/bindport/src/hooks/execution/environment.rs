use super::*;

#[derive(Debug)]
pub(crate) struct HookEnvironment {
    pub(crate) events: String,
    pub(crate) sources: String,
    pub(crate) context: String,
}

impl HookEnvironment {
    pub(crate) fn new(
        route_events: &RouteEventCollector,
        hook_events: &BTreeSet<HookEvent>,
    ) -> Self {
        Self {
            events: hook_events
                .iter()
                .map(|event| event.as_str())
                .collect::<Vec<_>>()
                .join(","),
            sources: route_events.hook_sources(),
            context: route_events.warning_context(),
        }
    }
}
