use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderCommand {
    Render,
    Help,
}

#[derive(Debug, Default)]
pub(crate) struct RenderCommandOptions {
    pub(crate) output: Option<String>,
    pub(crate) all: bool,
    pub(crate) dry_run: bool,
    pub(crate) diff: bool,
    pub(crate) repair: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderReport {
    Print,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderMode {
    Normal,
    Diff,
    Repair,
}

pub(crate) struct RenderInvocation<'a> {
    pub(crate) outputs: Vec<EffectiveOutputConfig>,
    pub(crate) dry_run: bool,
    pub(crate) mode: RenderMode,
    pub(crate) report: RenderReport,
    pub(crate) events: &'a RouteEventCollector,
}
