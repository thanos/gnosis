use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use gnosis_core::{Exporter, Pipeline, QueryEngine, ScanConfig};
use gnosis_okf::OkfExporter;
use gnosis_providers::default_registry;
use gnosis_tui::{drain_events_headless, TuiApp};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "gnosis",
    about = "Gnosis — enterprise knowledge compiler",
    long_about = "Compile a local repository into structured, traceable knowledge (OKF)."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Discover and analyze a local directory or Git repository
    Scan {
        /// Path to scan
        path: PathBuf,
        /// Run without the interactive TUI (for CI / scripting)
        #[arg(long)]
        no_tui: bool,
        /// Suppress per-object event lines in headless mode
        #[arg(long)]
        quiet: bool,
        /// Maximum object size in bytes
        #[arg(long, default_value_t = 2 * 1024 * 1024)]
        max_size: u64,
        /// Worker concurrency
        #[arg(long)]
        concurrency: Option<usize>,
        /// OKF export directory (created on `export okf`)
        #[arg(long, default_value = "knowledge.okf")]
        output: PathBuf,
        /// Automatically export OKF when the scan finishes (headless)
        #[arg(long)]
        export: bool,
    },
    /// Show product overview
    About,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::About => {
            println!("{HELP_TEXT}");
            Ok(())
        }
        Commands::Scan {
            path,
            no_tui,
            quiet,
            max_size,
            concurrency,
            output,
            export,
        } => run_scan(path, no_tui, quiet, max_size, concurrency, output, export),
    }
}

fn run_scan(
    path: PathBuf,
    no_tui: bool,
    quiet: bool,
    max_size: u64,
    concurrency: Option<usize>,
    output: PathBuf,
    auto_export: bool,
) -> Result<()> {
    if !path.exists() {
        bail!("path does not exist: {}", path.display());
    }

    let mut config = ScanConfig::with_root(
        path.canonicalize()
            .with_context(|| format!("canonicalize {}", path.display()))?,
    );
    config.max_object_size = max_size;
    if let Some(c) = concurrency {
        config.concurrency = c.max(1);
    }
    config.output_path = output.clone();

    let providers = default_registry();
    let pipeline = Pipeline::new(config.clone(), providers);
    let mut handle = pipeline.spawn();
    let events = handle.take_events();

    if no_tui {
        drain_events_headless(events, quiet);
        handle.wait().context("pipeline failed")?;

        let store = handle.store.lock().unwrap();
        let q = QueryEngine::new(&store);
        print!("{}", q.summary());

        if auto_export {
            let exporter = OkfExporter::new();
            let out = if output.is_absolute() {
                output.clone()
            } else {
                config.root.join(&output)
            };
            println!("exporting OKF to {} …", out.display());
            exporter.export(&store, &out).context("OKF export failed")?;
            println!("export complete");
        }

        let unknown = q.unknown();
        if !unknown.is_empty() {
            println!("\nunknown / partial ({}):", unknown.len());
            for o in unknown.iter().take(20) {
                println!(
                    "  [{}] {} — {}",
                    o.status,
                    o.descriptor.relative_path.display(),
                    o.classification_reason.as_deref().unwrap_or("")
                );
            }
        }
        return Ok(());
    }

    let export_path = if output.is_absolute() {
        output.clone()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| config.root.clone())
            .join(&output)
    };

    let mut app = TuiApp::new(
        config.root.clone(),
        handle.store.clone(),
        handle.metrics.clone(),
        events,
        config.event_history_len,
        export_path,
    );
    app.set_export_handler(|store, path| {
        OkfExporter::new()
            .export(store, path)
            .map_err(|e| anyhow::anyhow!(e))
    });

    let result = app.run();
    handle.cancel();
    thread::sleep(Duration::from_millis(50));
    let _ = handle.wait();
    result.context("TUI exited with error")?;
    Ok(())
}

const HELP_TEXT: &str = r#"Gnosis — Enterprise Knowledge Compiler

  gnosis scan <path>           Live TUI scan
  gnosis scan <path> --no-tui  Headless scan (prints summary)
  gnosis scan <path> --no-tui --export

TUI commands (press :):
  summary | objects [status] | unknown | providers | stats
  find <text> | explain <name> | graph <name>
  export okf [path] | help | quit

Shortcuts: s summary · u unknown · e export · q quit
"#;
