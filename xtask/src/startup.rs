// SPDX-License-Identifier: MIT

use std::{
    env,
    ffi::OsStr,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const WARMUP_PAIRS: usize = 5;
const SAMPLE_PAIRS: usize = 21;
const LOCAL_BUDGET: Duration = Duration::from_millis(50);
const CI_CEILING: Duration = Duration::from_millis(100);

pub fn startup_budget(ci: bool) -> Result<(), String> {
    let xtask = env::current_exe()
        .map_err(|error| format!("failed to locate the xtask executable: {error}"))?;
    let bin_dir = xtask
        .parent()
        .ok_or_else(|| format!("xtask executable has no parent: {}", xtask.display()))?;
    if bin_dir.file_name() != Some(OsStr::new("release")) {
        return Err(format!(
            "startup-budget requires release binaries; run `cargo build --release --locked` then `{}/xtask startup-budget`",
            bin_dir.with_file_name("release").display()
        ));
    }
    let bindport = bin_dir.join(format!("bindport{}", env::consts::EXE_SUFFIX));
    if !bindport.is_file() {
        return Err(format!(
            "release bindport binary not found at {}; run `cargo build --release --locked`",
            bindport.display()
        ));
    }

    let fixture = StartupFixture::new()?;
    for _ in 0..WARMUP_PAIRS {
        measure_startup(None, &xtask, &fixture)?;
        measure_startup(Some(&bindport), &xtask, &fixture)?;
    }

    let mut direct = Vec::with_capacity(SAMPLE_PAIRS);
    let mut wrapped = Vec::with_capacity(SAMPLE_PAIRS);
    let mut overhead = Vec::with_capacity(SAMPLE_PAIRS);
    for index in 0..SAMPLE_PAIRS {
        let (direct_sample, wrapped_sample) = if index % 2 == 0 {
            let direct_sample = measure_startup(None, &xtask, &fixture)?;
            let wrapped_sample = measure_startup(Some(&bindport), &xtask, &fixture)?;
            (direct_sample, wrapped_sample)
        } else {
            let wrapped_sample = measure_startup(Some(&bindport), &xtask, &fixture)?;
            let direct_sample = measure_startup(None, &xtask, &fixture)?;
            (direct_sample, wrapped_sample)
        };
        direct.push(direct_sample);
        wrapped.push(wrapped_sample);
        overhead.push(duration_nanos(wrapped_sample) - duration_nanos(direct_sample));
    }

    let median_direct = median_duration(&direct);
    let median_wrapped = median_duration(&wrapped);
    let median_overhead = median_i128(&overhead);
    let local_budget_nanos = duration_nanos(LOCAL_BUDGET);
    let required_ceiling = if ci { CI_CEILING } else { LOCAL_BUDGET };

    println!(
        "wrapper startup contract: elapsed `bindport -- <minimal child>` minus the same direct child, including process spawn/wait in both paths"
    );
    println!(
        "methodology: release binaries, isolated output-free/hook-free config and SQLite registry, {} warm-up pairs, {} alternating measured pairs, paired median",
        WARMUP_PAIRS, SAMPLE_PAIRS
    );
    println!("direct samples (ms):  {}", format_duration_samples(&direct));
    println!(
        "wrapped samples (ms): {}",
        format_duration_samples(&wrapped)
    );
    println!("overhead samples (ms): {}", format_nanos_samples(&overhead));
    println!(
        "medians (ms): direct={:.3} wrapped={:.3} overhead={:.3}",
        duration_ms(median_direct),
        duration_ms(median_wrapped),
        nanos_ms(median_overhead)
    );
    println!(
        "local budget: {:.3} ms ({})",
        duration_ms(LOCAL_BUDGET),
        if median_overhead <= local_budget_nanos {
            "met"
        } else {
            "exceeded"
        }
    );
    if ci {
        println!(
            "required noisy-runner regression ceiling: {:.3} ms",
            duration_ms(CI_CEILING)
        );
    }

    if median_overhead > duration_nanos(required_ceiling) {
        return Err(format!(
            "startup overhead median {:.3} ms exceeded the {} threshold {:.3} ms",
            nanos_ms(median_overhead),
            if ci { "CI regression" } else { "local budget" },
            duration_ms(required_ceiling)
        ));
    }

    println!(
        "startup overhead median {:.3} ms is within the {} threshold {:.3} ms",
        nanos_ms(median_overhead),
        if ci { "CI regression" } else { "local budget" },
        duration_ms(required_ceiling)
    );
    Ok(())
}

fn measure_startup(
    wrapper: Option<&Path>,
    child: &Path,
    fixture: &StartupFixture,
) -> Result<Duration, String> {
    let mut command = if let Some(wrapper) = wrapper {
        let mut command = Command::new(wrapper);
        command.arg("--").arg(child).arg("__startup-child");
        command
    } else {
        let mut command = Command::new(child);
        command.arg("__startup-child");
        command
    };
    command
        .current_dir(&fixture.root)
        .env("BINDPORT_REGISTRY_PATH", &fixture.registry)
        .env("XDG_CONFIG_HOME", fixture.root.join("config-home"))
        .env("XDG_STATE_HOME", fixture.root.join("state-home"))
        .env("HOME", fixture.root.join("home"))
        .env_remove("APPDATA")
        .env_remove("BINDPORT_PROJECT")
        .env_remove("BINDPORT_SERVICE")
        .env_remove("BINDPORT_HOSTNAME")
        .env_remove("BINDPORT_ROUTE_URL")
        .env_remove("BINDPORT_HEALTH_URL")
        .env_remove("BINDPORT_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let started = Instant::now();
    let status = command
        .status()
        .map_err(|error| format!("failed to execute startup sample: {error}"))?;
    let elapsed = started.elapsed();
    if !status.success() {
        return Err(format!("startup sample exited with {status}"));
    }

    Ok(elapsed)
}

struct StartupFixture {
    root: PathBuf,
    registry: PathBuf,
}

impl StartupFixture {
    fn new() -> Result<Self, String> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock is before the Unix epoch: {error}"))?
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "bindport-startup-budget-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root)
            .map_err(|error| format!("failed to create startup fixture: {error}"))?;
        let root = root.canonicalize().map_err(|error| {
            format!("failed to canonicalize startup fixture directory: {error}")
        })?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("failed to reserve a startup fixture port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("failed to read startup fixture port: {error}"))?
            .port();
        fs::write(
            root.join(".bindport.toml"),
            format!(
                "project = \"startup-budget\"\nservice = \"noop\"\ndefault_range = \"{port}-{port}\"\nskip_ports = []\n"
            ),
        )
        .map_err(|error| format!("failed to write startup fixture config: {error}"))?;
        drop(listener);
        let registry = root.join("registry.sqlite");

        Ok(Self { root, registry })
    }
}

impl Drop for StartupFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn median_duration(samples: &[Duration]) -> Duration {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn median_i128(samples: &[i128]) -> i128 {
    let mut samples = samples.to_vec();
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn duration_nanos(duration: Duration) -> i128 {
    duration.as_nanos() as i128
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn nanos_ms(nanos: i128) -> f64 {
    nanos as f64 / 1_000_000.0
}

fn format_duration_samples(samples: &[Duration]) -> String {
    let nanos = samples
        .iter()
        .map(|sample| duration_nanos(*sample))
        .collect::<Vec<_>>();
    format_nanos_samples(&nanos)
}

fn format_nanos_samples(samples: &[i128]) -> String {
    samples
        .iter()
        .map(|sample| format!("{:.3}", nanos_ms(*sample)))
        .collect::<Vec<_>>()
        .join(", ")
}
