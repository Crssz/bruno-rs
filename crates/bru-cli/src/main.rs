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
    /// Data file (`.json` array of objects or `.csv`); run the targets once per
    /// row, with each row's fields injected as variables for that iteration.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Number of iterations to run. Ignored when `--data` is given (the row
    /// count wins). Defaults to 1.
    #[arg(long)]
    iterations: Option<usize>,
    /// Developer Mode: let scripts `require()` local `.js` files (relative to the
    /// request). Off by default (Safe Mode — scripts have no filesystem access).
    #[arg(long)]
    developer: bool,
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

    // A dataset, if given, drives the iteration count and injects per-row vars.
    let dataset = match args.data.as_deref().map(load_dataset).transpose() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let iterations = match &dataset {
        Some(rows) => rows.len(),
        None => args.iterations.unwrap_or(1),
    };
    if let Some(rows) = &dataset {
        if rows.is_empty() {
            eprintln!("Error: data file has no rows");
            return ExitCode::FAILURE;
        }
    }

    let base = base_vars(&args.path, args.env.as_deref());
    let multi = iterations > 1 || dataset.is_some();

    let (mut passed, mut failed) = (0u32, 0u32);
    for iter in 0..iterations {
        if multi {
            println!("\n=== iteration {}/{iterations} ===", iter + 1);
        }
        // Each iteration starts from the base vars (collection + env), overlaid
        // with this row's data, in a fresh context so iterations don't leak vars.
        let mut vars = base.clone();
        if let Some(rows) = &dataset {
            for (k, v) in &rows[iter] {
                vars.insert(k.clone(), v.clone());
            }
        }
        let mut ctx = RunContext {
            vars,
            client: client.clone(),
            send_options: options.clone(),
            developer_mode: args.developer,
            ..Default::default()
        };

        for target in &targets {
            // Resolve each request's `require('./x')` relative to its own folder.
            ctx.script_dir = target.parent().map(Path::to_path_buf);
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
    }

    if multi {
        println!("\n{iterations} iterations, {passed} passed, {failed} failed");
    } else {
        println!("\n{passed} passed, {failed} failed");
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A dataset is a list of iterations; each iteration is a list of key/value vars.
type Dataset = Vec<Vec<(String, String)>>;

/// Load a data file as a list of per-iteration variable rows. JSON files must be
/// an array of objects; CSV files use the header row as keys. All values are
/// stringified (JSON numbers/bools become their literal text; strings drop their
/// quotes).
fn load_dataset(path: &Path) -> Result<Dataset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let is_csv = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    if is_csv {
        load_csv(&text)
    } else {
        load_json(&text)
    }
}

fn load_json(text: &str) -> Result<Dataset, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("data: invalid JSON: {e}"))?;
    let arr = value
        .as_array()
        .ok_or("data: JSON must be an array of objects")?;
    let mut rows = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("data: row {i} is not an object"))?;
        let row = obj
            .iter()
            .map(|(k, v)| (k.clone(), json_to_string(v)))
            .collect();
        rows.push(row);
    }
    Ok(rows)
}

/// Stringify a JSON value for use as a variable: strings unquoted, scalars by
/// their literal text, and any nested object/array by its compact JSON.
fn json_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn load_csv(text: &str) -> Result<Dataset, String> {
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("data: invalid CSV header: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| format!("data: invalid CSV row: {e}"))?;
        let row = headers
            .iter()
            .cloned()
            .zip(record.iter().map(|s| s.to_string()))
            .collect();
        rows.push(row);
    }
    Ok(rows)
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
