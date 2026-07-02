// SPDX-License-Identifier: MIT

use super::*;

pub fn bindport() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bindport"))
}

pub fn bindport_with_registry(registry_path: &Path) -> Command {
    let mut command = bindport();
    command.env(REGISTRY_PATH_ENV, registry_path);
    command.env("XDG_CONFIG_HOME", config_home_for_registry(registry_path));
    command.env("XDG_STATE_HOME", state_home_for_registry(registry_path));
    command.env_remove(BINDPORT_PROJECT_ENV);
    command.env_remove(BINDPORT_SERVICE_ENV);
    command
}

pub fn bindport_without_registry_path() -> Command {
    let mut command = bindport();
    command.env_remove(REGISTRY_PATH_ENV);
    command.env_remove("XDG_CONFIG_HOME");
    command.env_remove("XDG_STATE_HOME");
    command.env_remove("HOME");
    command.env_remove("APPDATA");
    command
}

pub fn config_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("config-home")
}

pub fn state_home_for_registry(registry_path: &Path) -> PathBuf {
    registry_path.with_extension("state-home")
}

pub fn run_print_port(registry_path: &Path, cwd: &Path) -> u16 {
    let output = bindport_with_registry(registry_path)
        .current_dir(cwd)
        .args(["--", "sh", "-c", "printf '%s' \"$PORT\""])
        .output()
        .expect("run bindport");

    assert!(output.status.success());

    String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .parse::<u16>()
        .expect("stdout is a port number")
}
