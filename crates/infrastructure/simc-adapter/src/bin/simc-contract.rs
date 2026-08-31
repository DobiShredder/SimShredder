use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};
#[cfg(target_os = "macos")]
use simc_adapter::discover_latest_macos;
use simc_adapter::{
    RuntimeManifest, check_artifact_availability, discover_latest_supported, download_available,
    download_verified, install_supported_artifact, run_benchmark, run_executable_contract,
    validate_supported_binary,
};

#[derive(Debug, Parser)]
#[command(about = "Phase 0A SimulationCraft runtime contract tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[cfg(target_os = "macos")]
    Discover {
        listing_file: PathBuf,
    },
    Download {
        manifest: PathBuf,
        directory: PathBuf,
    },
    Availability {
        manifest: PathBuf,
    },
    Latest {
        directory: PathBuf,
        manifest: PathBuf,
    },
    Install {
        manifest: PathBuf,
        dmg: PathBuf,
        install_root: PathBuf,
    },
    Validate {
        executable: PathBuf,
    },
    Contract {
        executable: PathBuf,
        quick_fixture: PathBuf,
        profileset_fixture: PathBuf,
        quick_golden: PathBuf,
        profileset_golden: PathBuf,
        output_directory: PathBuf,
    },
    Benchmark {
        executable: PathBuf,
        profileset_fixture: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        iterations: usize,
        #[arg(long, default_value_t = 3)]
        repetitions: usize,
    },
}

fn read_manifest(path: &PathBuf) -> anyhow_free::Result<RuntimeManifest> {
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn main() -> anyhow_free::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        #[cfg(target_os = "macos")]
        Commands::Discover { listing_file } => {
            let html = fs::read_to_string(listing_file)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&discover_latest_macos(&html)?)?
            );
        }
        Commands::Download {
            manifest,
            directory,
        } => {
            let manifest = read_manifest(&manifest)?;
            println!("{}", download_verified(&manifest, &directory)?.display());
        }
        Commands::Availability { manifest } => {
            let manifest = read_manifest(&manifest)?;
            check_artifact_availability(&manifest)?;
            println!("available {}", manifest.filename);
        }
        Commands::Latest {
            directory,
            manifest,
        } => {
            let available = discover_latest_supported()?;
            let (resolved, artifact) = download_available(&available, &directory)?;
            let mut bytes = serde_json::to_vec_pretty(&resolved)?;
            bytes.push(b'\n');
            fs::write(manifest, bytes)?;
            println!("{}", artifact.display());
        }
        Commands::Install {
            manifest,
            dmg,
            install_root,
        } => {
            let manifest = read_manifest(&manifest)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&install_supported_artifact(
                    &manifest,
                    &dmg,
                    &install_root,
                )?)?
            );
        }
        Commands::Validate { executable } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&validate_supported_binary(&executable)?)?
            );
        }
        Commands::Contract {
            executable,
            quick_fixture,
            profileset_fixture,
            quick_golden,
            profileset_golden,
            output_directory,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&run_executable_contract(
                    &executable,
                    &quick_fixture,
                    &profileset_fixture,
                    &quick_golden,
                    &profileset_golden,
                    &output_directory,
                )?)?
            );
        }
        Commands::Benchmark {
            executable,
            profileset_fixture,
            output,
            iterations,
            repetitions,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&run_benchmark(
                    &executable,
                    &profileset_fixture,
                    &output,
                    iterations,
                    repetitions,
                )?)?
            );
        }
    }
    Ok(())
}

mod anyhow_free {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}
