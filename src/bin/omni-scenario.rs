use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use omni_proxy::{
    faults::{FaultSpec, run_with_fault},
    replay::load_capture,
    scenario::{Scenario, from_capture, junit},
};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Portable HTTP scenarios, reproducible faults and CI regression reports"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Validate {
        file: PathBuf,
    },
    Run {
        file: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long = "var", value_name = "NAME=JSON")]
        variables: Vec<String>,
        #[arg(long)]
        fault: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        junit: Option<PathBuf>,
    },
    Capture {
        file: PathBuf,
        #[arg(long, value_delimiter = ',', required = true)]
        indexes: Vec<usize>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        output: PathBuf,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Validate { file } => {
            let scenario: Scenario = serde_json::from_slice(&std::fs::read(file)?)?;
            scenario.validate()?;
            println!("valid: {} ({} steps)", scenario.name, scenario.steps.len());
        }
        Command::Capture {
            file,
            indexes,
            name,
            output,
        } => {
            let scenario = from_capture(&load_capture(&file)?, &indexes, name)?;
            std::fs::write(output, serde_json::to_vec_pretty(&scenario)?)?;
        }
        Command::Run {
            file,
            target,
            variables,
            fault,
            report,
            junit: xml,
        } => {
            let scenario: Scenario = serde_json::from_slice(&std::fs::read(file)?)?;
            let mut overrides = BTreeMap::new();
            for variable in variables {
                let (name, value) = variable
                    .split_once('=')
                    .context("--var requires NAME=JSON (quote string values as JSON)")?;
                overrides.insert(
                    name.into(),
                    serde_json::from_str::<Value>(value).context("variable must be valid JSON")?,
                );
            }
            let fault: Option<FaultSpec> = fault
                .map(|path| -> Result<_> { Ok(serde_json::from_slice(&std::fs::read(path)?)?) })
                .transpose()?;
            let result = run_with_fault(&scenario, target.as_deref(), overrides, fault).await?;
            let json = serde_json::to_string_pretty(&result)?;
            if let Some(path) = report {
                std::fs::write(path, &json)?;
            }
            if let Some(path) = xml {
                std::fs::write(path, junit(&result))?;
            }
            println!("{json}");
            if result.failed > 0 {
                bail!("regression failed: {} failed step(s)", result.failed);
            }
        }
    }
    Ok(())
}
