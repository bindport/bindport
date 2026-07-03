use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteEventSource {
    CliRunner,
    CliReserve,
    CliClean,
    DashboardClean,
    ManualRender,
    StaleReconcile,
}

impl RouteEventSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::CliRunner => "cli_runner",
            Self::CliReserve => "cli_reserve",
            Self::CliClean => "cli_clean",
            Self::DashboardClean => "dashboard_clean",
            Self::ManualRender => "manual_render",
            Self::StaleReconcile => "stale_reconcile",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RouteEventKind {
    RouteStarted,
    RouteFinished,
    RoutesRemoved,
    RoutesMarkedStale,
    RenderRequested,
}

impl RouteEventKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RouteStarted => "route_started",
            Self::RouteFinished => "route_finished",
            Self::RoutesRemoved => "routes_removed",
            Self::RoutesMarkedStale => "routes_marked_stale",
            Self::RenderRequested => "render_requested",
        }
    }
}

impl From<RouteEventKind> for HookEvent {
    fn from(kind: RouteEventKind) -> Self {
        match kind {
            RouteEventKind::RouteStarted => Self::RouteStarted,
            RouteEventKind::RouteFinished => Self::RouteFinished,
            RouteEventKind::RoutesRemoved => Self::RoutesRemoved,
            RouteEventKind::RoutesMarkedStale => Self::RoutesMarkedStale,
            RouteEventKind::RenderRequested => Self::RenderRequested,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RouteEvent {
    pub(crate) source: RouteEventSource,
    pub(crate) kind: RouteEventKind,
}

impl RouteEvent {
    pub(crate) const fn new(source: RouteEventSource, kind: RouteEventKind) -> Self {
        Self { source, kind }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RouteEventCollector {
    pub(crate) events: Vec<RouteEvent>,
}
impl RouteEventCollector {
    pub(crate) fn single(source: RouteEventSource, kind: RouteEventKind) -> Self {
        let mut collector = Self::default();
        collector.record(source, kind);
        collector
    }

    pub(crate) fn record(&mut self, source: RouteEventSource, kind: RouteEventKind) {
        self.events.push(RouteEvent::new(source, kind));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events().is_empty()
    }

    pub(crate) fn warning_context(&self) -> String {
        match self.events.as_slice() {
            [event] => format!("{} {}", event.source.as_str(), event.kind.as_str()),
            [] => String::from("route event"),
            events => {
                let sources = events
                    .iter()
                    .map(|event| event.source.as_str())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(",");
                format!("route events from {sources}")
            }
        }
    }

    pub(crate) fn events(&self) -> &[RouteEvent] {
        &self.events
    }

    pub(crate) fn hook_events(&self, output_rendered: bool) -> BTreeSet<HookEvent> {
        let mut events = self
            .events
            .iter()
            .map(|event| HookEvent::from(event.kind))
            .collect::<BTreeSet<_>>();

        if output_rendered {
            events.insert(HookEvent::OutputRendered);
        }

        events
    }

    pub(crate) fn hook_sources(&self) -> String {
        self.events
            .iter()
            .map(|event| event.source.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(",")
    }
}
