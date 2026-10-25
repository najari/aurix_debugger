//! `aurix-run`: runs a board test script against the probe-rs DAP server.
//!
//! ```text
//! aurix-run <script> [--server HOST:PORT] [--keep-going] [--junit FILE] [--set NAME=VALUE]... [--verbose]
//! ```
//!
//! Exit code 0: every expectation held; 1: one did not; 2: the script has a mistake, the server
//! could not be reached or stopped answering, or a wait ran out of time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aurix_script::run::{Options, run};
use aurix_script::script::Script;

const USAGE: &str = "usage: aurix-run <script> [--server HOST:PORT] [--keep-going] [--junit FILE] [--set NAME=VALUE]... [--verbose]";

struct Args {
    script: PathBuf,
    junit: Option<PathBuf>,
    vars: HashMap<String, String>,
    options: Options,
}

fn parse_args() -> Result<Args, String> {
    let mut args = std::env::args().skip(1);
    let mut script = None;
    let mut junit = None;
    let mut vars = HashMap::new();
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--server" => options.server = Some(value("--server")?),
            "--keep-going" => options.keep_going = true,
            "--verbose" => options.verbose = true,
            "--junit" => junit = Some(PathBuf::from(value("--junit")?)),
            "--set" => {
                let pair = value("--set")?;
                let (name, value) = pair
                    .split_once('=')
                    .ok_or(format!("--set {pair}: write NAME=VALUE"))?;
                vars.insert(name.to_string(), value.to_string());
            }
            "-h" | "--help" => return Err(String::new()),
            other if other.starts_with("--") => return Err(format!("{other} is not an option")),
            other if script.is_none() => script = Some(PathBuf::from(other)),
            other => return Err(format!("{other}: one script at a time")),
        }
    }
    Ok(Args {
        script: script.ok_or_else(String::new)?,
        junit,
        vars,
        options,
    })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let name = args
        .script
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| args.script.display().to_string());
    let text = match std::fs::read_to_string(&args.script) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{}: {error}", args.script.display());
            return ExitCode::from(2);
        }
    };
    let script = match Script::parse(&text, &args.vars) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("{name}:{}: {}", error.line, error.message);
            return ExitCode::from(2);
        }
    };

    // Ctrl+C ends the run at the next chance, with a disconnect.
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = interrupted.clone();
        ctrlc::set_handler(move || interrupted.store(true, Ordering::SeqCst)).ok();
    }
    let options = Options {
        interrupted,
        ..args.options
    };

    let base_dir = args
        .script
        .parent()
        .map(Path::to_path_buf)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."));
    println!("{name}: {} command(s)", script.lines.len());
    let mut out = std::io::stdout();
    let outcome = run(&script, &base_dir, &options, &mut out);

    let failed: Vec<_> = outcome
        .cases
        .iter()
        .filter(|c| c.failure.is_some())
        .collect();
    let passed = outcome.cases.len() - failed.len();
    let total = script.expectations().count();
    println!(
        "{name}: {passed} of {total} expectation(s) held, {} failed, {:.2}s",
        failed.len(),
        outcome.time.as_secs_f64()
    );
    if options.keep_going && !failed.is_empty() {
        for case in &failed {
            println!(
                "  {name}:{}: {}: {}",
                case.line,
                case.text,
                case.failure.as_deref().unwrap_or_default()
            );
        }
    }
    if let Some(error) = &outcome.error {
        let at = error.line.map(|l| format!(":{l}")).unwrap_or_default();
        println!("{name}{at}: error: {}", error.message);
    }
    if let Some(path) = &args.junit
        && let Err(error) =
            std::fs::write(path, aurix_script::junit::report(&name, &script, &outcome))
    {
        eprintln!("{}: {error}", path.display());
        return ExitCode::from(2);
    }
    ExitCode::from(outcome.exit_code() as u8)
}
