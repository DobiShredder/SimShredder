use std::{fs, path::PathBuf, time::Duration};

use clap::{Parser, ValueEnum};
use simshredder_core::{InputFormat, execute, prepare};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    Addon,
    Simc,
}

#[derive(Debug, Parser)]
#[command(about = "Prepare and execute one SimShredder Quick Sim run")]
struct Cli {
    #[arg(long, value_enum)]
    format: Format,
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    executable: PathBuf,
    #[arg(long)]
    revision: String,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 300)]
    timeout_seconds: u64,
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let source = fs::read_to_string(&cli.source)?;
    let format = match cli.format {
        Format::Addon => InputFormat::AddonExport,
        Format::Simc => InputFormat::SimcFile,
    };
    let prepared = prepare(&source, format)?;
    let result = execute(
        &prepared,
        &cli.executable,
        &cli.revision,
        &cli.output,
        Duration::from_secs(cli.timeout_seconds),
    )?;
    println!("{}", serde_json::to_string_pretty(&result.normalized)?);
    eprintln!("artifacts: {}", result.directory.display());
    Ok(())
}
