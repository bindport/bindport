use super::*;

pub(crate) fn run_init_command(args: &[String]) -> ExitCode {
    match InitTarget::parse(args) {
        Ok(InitTarget::Project) => init_project_config(),
        Ok(InitTarget::User) => init_user_config(),
        Ok(InitTarget::Help) => {
            print_init_help();
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("bindport: {message}");
            eprintln!("usage: bindport init [--project|--user]");
            ExitCode::FAILURE
        }
    }
}

enum InitTarget {
    Project,
    User,
    Help,
}

impl InitTarget {
    fn parse(args: &[String]) -> Result<Self, String> {
        match args {
            [] => Ok(Self::Project),
            [flag] if flag == "--project" => Ok(Self::Project),
            [flag] if flag == "--user" => Ok(Self::User),
            [flag] if flag == "--help" || flag == "-h" => Ok(Self::Help),
            [flag, ..] if flag == "--project" || flag == "--user" => Err(format!(
                "init flag `{flag}` cannot be combined with other arguments"
            )),
            [flag] => Err(format!("unknown init option `{flag}`")),
            _ => Err(String::from("init accepts at most one option")),
        }
    }
}

pub(crate) enum InitConfigResult {
    Created(PathBuf),
    AlreadyExists(PathBuf),
}

fn init_project_config() -> ExitCode {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("bindport: failed to determine current directory: {error}");
            return ExitCode::FAILURE;
        }
    };

    match write_project_config(&cwd) {
        Ok(InitConfigResult::Created(path)) => {
            println!("created project config: {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(InitConfigResult::AlreadyExists(path)) => {
            println!("project config already exists: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: failed to initialize project config: {error}");
            ExitCode::FAILURE
        }
    }
}

fn init_user_config() -> ExitCode {
    match write_fallback_config() {
        Ok(InitConfigResult::Created(path)) => {
            println!("created user config: {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(InitConfigResult::AlreadyExists(path)) => {
            println!("user config already exists: {}", path.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bindport: failed to initialize user config: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn write_project_config(cwd: &Path) -> io::Result<InitConfigResult> {
    for filename in CONFIG_FILENAMES {
        let path = cwd.join(filename);
        match path.symlink_metadata() {
            Ok(metadata) if metadata.is_file() => return Ok(InitConfigResult::AlreadyExists(path)),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("`{}` exists but is not a regular file", path.display()),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }

    let path = cwd.join(".bindport.toml");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(default_project_config(cwd).as_bytes())?;

    Ok(InitConfigResult::Created(path))
}

pub(crate) fn write_fallback_config() -> io::Result<InitConfigResult> {
    let path = fallback_config_path()?;

    if path.is_file() {
        return Ok(InitConfigResult::AlreadyExists(path));
    }

    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("`{}` exists but is not a file", path.display()),
        ));
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(&path, default_fallback_config())?;

    Ok(InitConfigResult::Created(path))
}

fn default_project_config(cwd: &Path) -> String {
    let project = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .map(normalize_branch_label)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| String::from("bindport-project"));
    let skip_ports = DEFAULT_SKIP_PORTS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "# BindPort project config. Commit this file with your project.\n\
         # Machine-local overrides belong in .bindport.local.toml, which should stay untracked.\n\
         project = \"{project}\"\n\
         default_range = \"{}-{}\"\n\
         skip_ports = [{skip_ports}]\n",
        DEFAULT_PORT_RANGE.start, DEFAULT_PORT_RANGE.end
    )
}
