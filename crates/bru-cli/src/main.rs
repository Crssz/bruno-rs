//! `bru` — headless CLI for running Bruno collections.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::{HttpClient, SendOptions};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bru",
    version,
    about = "Run Bruno API collections from the command line"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a request file or a whole collection directory.
    Run(RunArgs),
}

#[derive(Parser)]
struct RunArgs {
    /// Path to a `.bru` request file or a collection directory.
    path: PathBuf,
    /// Environment name to load from `<collection>/environments/<name>.bru`.
    #[arg(long)]
    env: Option<String>,
    /// Skip TLS certificate verification.
    #[arg(long)]
    insecure: bool,
    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(args).await,
    }
}

async fn run(args: RunArgs) -> ExitCode {
    let targets = match collect_targets(&args.path) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            eprintln!("No requests found at {}", args.path.display());
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.insecure {
        eprintln!(
            "WARNING: TLS certificate verification is DISABLED for all requests (--insecure)."
        );
    }
    let options = SendOptions {
        insecure: args.insecure,
        timeout: Duration::from_secs(args.timeout),
        ..SendOptions::default()
    };
    let client = match HttpClient::new(&options) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut ctx = RunContext {
        vars: base_vars(&args.path, args.env.as_deref()),
        client,
        ..Default::default()
    };

    let (mut passed, mut failed) = (0u32, 0u32);
    for target in &targets {
        let text = match std::fs::read_to_string(target) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("skip {}: {e}", target.display());
                failed += 1;
                continue;
            }
        };
        let file = match bru_lang::parse(&text) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("skip {}: parse error: {e}", target.display());
                failed += 1;
                continue;
            }
        };
        let outcome = run_request(&file, &mut ctx).await;
        if outcome.passed() {
            passed += 1;
        } else {
            failed += 1;
        }
        print_outcome(&outcome);
    }

    println!("\n{passed} passed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Resolve which `.bru` files to run: a single file, or every request in a
/// collection directory (in sidebar order).
fn collect_targets(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_dir() {
        let tree = bru_lang::load_collection(path)?;
        let mut out = Vec::new();
        flatten(&tree.root, &mut out);
        Ok(out)
    } else {
        Ok(vec![path.to_path_buf()])
    }
}

fn flatten(folder: &bru_core::Folder, out: &mut Vec<PathBuf>) {
    for req in &folder.requests {
        out.push(req.path.clone());
    }
    for sub in &folder.folders {
        flatten(sub, out);
    }
}

fn print_outcome(o: &RunOutcome) {
    println!("\n> {}  {} {}", o.name, o.method, o.url);
    if let Some(err) = &o.error {
        println!("  ERROR: {err}");
        return;
    }
    if let Some(resp) = &o.response {
        println!(
            "  {} {}   {} ms   {} bytes",
            resp.status,
            resp.status_text,
            resp.duration_ms,
            resp.body.len()
        );
    }
    for a in &o.assertions {
        let mark = if a.passed { "PASS" } else { "FAIL" };
        if a.passed {
            println!("  [{mark}] {} {} {}", a.expr, a.operator, a.expected);
        } else {
            println!(
                "  [{mark}] {} {} {}  (actual: {})",
                a.expr, a.operator, a.expected, a.actual
            );
        }
    }
    for t in &o.tests {
        let mark = if t.passed { "PASS" } else { "FAIL" };
        match &t.error {
            Some(e) if !t.passed => println!("  [{mark}] test: {}  ({e})", t.name),
            _ => println!("  [{mark}] test: {}", t.name),
        }
    }
    for line in &o.console {
        println!("  | {line}");
    }
    for (k, v) in &o.vars_set {
        println!("  set var {k} = {}", redact(k, v));
    }
}

/// Mask values captured into secret-looking variable names so tokens don't land
/// in CI logs or shell history.
fn redact(name: &str, value: &str) -> String {
    const SECRETISH: &[&str] = &["token", "secret", "password", "passwd", "key", "auth"];
    let lower = name.to_lowercase();
    if SECRETISH.iter().any(|s| lower.contains(s)) {
        "***".to_string()
    } else {
        value.to_string()
    }
}
