pub(crate) fn print_help() {
    println!("BindPort - proxy-neutral local development port registry");
    println!();
    println!("Usage:");
    println!("  bindport -- <command>        Run a command with an assigned PORT");
    println!("  bindport run [service] [options] [-- <command>]");
    println!("                                  Run a command or configured service command");
    println!("  bindport reserve [service]     Hold a port without running a child process");
    println!("  bindport release [service|port]");
    println!("                                  Release a reserved port");
    println!("  bindport status [--json]     Show registry status");
    println!("  bindport open [service]      Print or open the best service URL");
    println!("  bindport clean [--dry-run]   Remove stopped and stale registry entries");
    println!("  bindport config explain      Explain resolved config and identity sources");
    println!("  bindport config validate     Validate config structure");
    println!("  bindport hooks status        Inspect configured hook trust");
    println!("  bindport doctor              Show bootstrap diagnostics");
    println!("  bindport doctor outputs      Validate output rendering setup");
    println!("  bindport dashboard [serve]   Serve the local dashboard");
    println!("  bindport dashboard start     Start the dashboard in the background");
    println!("  bindport dashboard status    Show background dashboard status");
    println!("  bindport dashboard stop      Stop the background dashboard");
    println!("  bindport render [output]     Render configured output files");
    println!("  bindport templates list      List resolved output templates");
    println!("  bindport templates show      Show a resolved output template");
    println!("  bindport templates export    Export a resolved output template");
    println!("  bindport init                Create project config in the current directory");
    println!("  bindport --version           Print version");
    println!();
    println!("Run options:");
    println!("  --env NAME=VALUE             Add a templated child environment variable");
    println!("  --hostname <template>        Set route hostname metadata");
    println!("  --route-url <template>       Set route URL metadata");
    println!("  --health-url <template>      Set service health check URL metadata");
}

pub(crate) fn print_reserve_help() {
    println!("BindPort lease reservation");
    println!();
    println!("Usage:");
    println!("  bindport reserve [service] [options]");
    println!("  bindport release [service|port]");
    println!();
    println!("Reserve options:");
    println!("  --hostname <template>     Set route hostname metadata");
    println!("  --route-url <template>    Set route URL metadata");
    println!("  --health-url <template>   Set service health check URL metadata");
}

pub(crate) fn print_init_help() {
    println!("BindPort config initialization");
    println!();
    println!("Usage:");
    println!("  bindport init [--project|--user]");
    println!();
    println!("Options:");
    println!("  --project    Create .bindport.toml in the current directory (default)");
    println!("  --user       Create optional user fallback config");
}

pub(crate) fn print_open_help() {
    println!("BindPort service URL lookup");
    println!();
    println!("Usage:");
    println!("  bindport open [service] [--project PROJECT] [--browser] [--print]");
    println!();
    println!("Options:");
    println!("  --project <project>    Disambiguate services with the same name");
    println!("  --browser              Open the URL with the system browser and print it");
    println!("  --print                Print the URL without launching a browser (default)");
}

pub(crate) fn print_config_help() {
    println!("BindPort config");
    println!();
    println!("Usage:");
    println!("  bindport config explain");
    println!("  bindport config validate");
    println!();
    println!("Commands:");
    println!("  explain    Show resolved config fields and identity sources");
    println!("  validate   Validate config structure and output actionable errors");
}

pub(crate) fn print_doctor_help() {
    println!("BindPort diagnostics");
    println!();
    println!("Usage:");
    println!("  bindport doctor");
    println!("  bindport doctor outputs");
    println!();
    println!("Commands:");
    println!("  outputs    Validate output config, templates, and planned file paths");
}

pub(crate) fn print_render_help() {
    println!("BindPort output rendering");
    println!();
    println!("Usage:");
    println!("  bindport render [output] [options]");
    println!();
    println!("Options:");
    println!("  --all        Render every enabled output (default)");
    println!("  --dry-run    Render templates and print targets without writing files");
    println!("  --diff       Print content changes without writing files");
    println!("  --repair     Re-render current routes and reconcile DB-owned files");
}

pub(crate) fn print_templates_help() {
    println!("BindPort output templates");
    println!();
    println!("Usage:");
    println!("  bindport templates list [--source project|global|built-in]");
    println!("  bindport templates show [--source project|global|built-in] <name>");
    println!("  bindport templates export [--source project|global|built-in] <name>");
    println!();
    println!("Options:");
    println!("  --source <source>    Resolve only project, global, or built-in templates");
}

pub(crate) fn print_clean_help() {
    println!("BindPort registry cleanup");
    println!();
    println!("Usage:");
    println!("  bindport clean [options]");
    println!();
    println!("Options:");
    println!("  --dry-run     Show what would be removed without deleting entries");
    println!("  --stopped     Remove stopped entries only");
    println!("  --stale       Remove stale entries only");
    println!("  --all         Remove stopped and stale entries (default)");
    println!("  --json        Print machine-readable cleanup counts");
    println!("  --yes, -y     Confirm stale entry deletion without prompting");
}

pub(crate) fn print_hooks_help() {
    println!("BindPort hooks");
    println!();
    println!("Usage:");
    println!("  bindport hooks status");
    println!("  bindport hooks trust [--scope worktree|repo] <hook|--all>");
    println!("  bindport hooks deny [--scope worktree|repo] <hook|--all>");
    println!("  bindport hooks reset [--scope worktree|repo] <hook|--all>");
    println!();
    println!("Options:");
    println!("  --scope <scope>    Trust scope, either worktree (default) or repo");
    println!("  --all              Select every configured hook");
}

pub(crate) fn print_dashboard_help() {
    println!("BindPort dashboard");
    println!();
    println!("Usage:");
    println!("  bindport dashboard [serve] [options]");
    println!("  bindport dashboard start [options]");
    println!("  bindport dashboard status");
    println!("  bindport dashboard stop");
    println!();
    println!("Options:");
    println!("  --host <ip>              Bind IP address (default 127.0.0.1)");
    println!("  --port <port>            Preferred dashboard port (default 27080)");
    println!("  --auth <mode>            required or disabled");
    println!("  --auth-required          Require bearer token access to dashboard data");
    println!("  --no-auth                Disable dashboard bearer token checks");
    println!("  --register-service       Record the dashboard in BindPort status");
    println!("  --no-register-service    Do not record the dashboard in BindPort status");
    println!("  --token <token>          Bearer token value (visible in process lists)");
    println!("  --token-env <name>       Environment variable containing the token");
    println!("  --allowed-host <host>    Additional accepted HTTP Host header");
    println!("  --static-dir <path>      Read dashboard assets from a local directory");
}
