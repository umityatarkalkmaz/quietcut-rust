//! quietcut command line entry point.

#![forbid(unsafe_code)]

use std::io::Write as _;
use std::process::ExitCode;

use clap::Parser;

use quietcut::cli::{Cli, Command, DetectionArgs};
use quietcut::error::Result;
use quietcut::{exec, pipeline, report};

fn main() -> ExitCode {
    match run_command() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run_command() -> Result<()> {
    let cli = Cli::parse();
    exec::verify_tools_available()?;

    match &cli.command {
        Command::Analyze(args) => run_analyze(args),
    }
}

fn run_analyze(args: &DetectionArgs) -> Result<()> {
    let config = args.build_detection_config()?;
    let analysis = pipeline::analyze_media(&config)?;

    for warning in &analysis.warnings {
        eprintln!("quietcut: warning: {warning}");
    }

    let mut stdout = std::io::stdout().lock();
    let _ = write!(stdout, "{}", report::render_analysis(&analysis, &config));
    Ok(())
}

/// Prints the error and its source chain to stderr.
fn report_error(error: &dyn std::error::Error) {
    eprintln!("quietcut: error: {error}");
    let mut source = error.source();
    while let Some(current) = source {
        eprintln!("  caused by: {current}");
        source = current.source();
    }
}
