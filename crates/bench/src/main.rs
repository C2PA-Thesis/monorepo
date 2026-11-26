//! Benchmarks the proof pipeline. `run` takes the samples, each in a process
//! of its own, and writes a report. See crates/bench/README.md.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use provenance::Workspace;

mod baseline;
mod environment;
mod measure;
mod phone;
mod probe;
mod report;
mod run;
mod stats;
mod suite;
mod workload;

use environment::Environment;
use run::{Paths, RunInfo};
use suite::{scenarios, Depth, Group, Scenario};
use workload::{Lab, Task};

#[derive(Parser)]
#[command(about = "Benchmark the proof pipeline and write a report")]
struct Cli {
    /// Where the location prover, proof parameters and keys live.
    #[arg(
        long,
        global = true,
        env = "PROVENANCE_HOME",
        default_value = ".provenance"
    )]
    home: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Take the samples and write the report.
    Run {
        /// Two samples of the reference operations only: a check, not a measurement.
        #[arg(long, conflicts_with_all = ["group", "resume"])]
        quick: bool,
        /// Run only this group. Repeat for several. Defaults to all of them.
        #[arg(long, value_enum)]
        group: Vec<Group>,
        /// Samples per scenario, instead of the suite's own counts.
        #[arg(long)]
        samples: Option<u32>,
        /// Kill a sample after this many minutes.
        #[arg(long, default_value_t = 15)]
        timeout_minutes: u64,
        /// A note to keep with the run.
        #[arg(long)]
        label: Option<String>,
        /// Continue the run with this id, from where it stopped.
        #[arg(long, conflicts_with_all = ["group", "samples", "label"])]
        resume: Option<String>,
    },
    /// Write the report of a run again from its samples.
    Report {
        /// The run's directory.
        dir: PathBuf,
    },
    /// Print what `run` would measure.
    List {
        #[arg(long)]
        quick: bool,
    },
    /// Run one operation and print what it measured. `run` calls this.
    #[command(hide = true)]
    Sample {
        #[arg(long)]
        work: PathBuf,
        task: String,
    },
    /// Prove one crop under the memory probe and print what it measured.
    #[command(hide = true)]
    Probe {
        #[arg(long)]
        work: PathBuf,
        /// Check the proof after measuring.
        #[arg(long)]
        verify: bool,
        /// Write live heap bytes and footprint over time here, as CSV.
        #[arg(long)]
        trace: Option<PathBuf>,
        /// Write the proof here.
        #[arg(long)]
        proof_out: Option<PathBuf>,
        /// The case, as in a sample's task. Defaults to the reference case.
        case: Option<String>,
    },
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn results_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("results")
}

fn plan(quick: bool, groups: Vec<Group>, samples: Option<u32>) -> Vec<Scenario> {
    let depth = match (samples, quick) {
        (Some(samples), _) => Depth::uniform(samples),
        (None, true) => Depth::QUICK,
        (None, false) => Depth::FULL,
    };
    let groups = match (quick, groups.is_empty()) {
        (true, _) => vec![Group::Pipeline],
        (false, true) => Group::ALL.to_vec(),
        (false, false) => groups,
    };
    scenarios(&groups, depth)
}

/// `20261106T150000Z-01046cbe`, like a capture directory.
fn new_id(environment: &Environment) -> String {
    let stamp: String = run::now()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    let commit = environment.commit.as_deref().unwrap_or("unknown");
    format!("{stamp}-{}", &commit[..commit.len().min(8)])
}

fn check_setup(workspace: &Workspace) -> Result<()> {
    workspace
        .verifier()
        .and_then(|_| workspace.device_key())
        .context("the proof parameters are missing; run scripts/bench.sh, or `provenance setup`")?;
    ensure!(
        workspace.sample_photo().exists(),
        "the sample photo is missing; run `provenance setup`"
    );
    Ok(())
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<()> {
    let home = std::path::absolute(&cli.home)?;
    match cli.command {
        Command::Run {
            quick,
            group,
            samples,
            timeout_minutes,
            label,
            resume,
        } => {
            ensure!(
                !cfg!(debug_assertions),
                "a debug build measures the wrong thing; use scripts/bench.sh or --release"
            );
            let workspace = Workspace::new(&home);
            check_setup(&workspace)?;
            let info = match &resume {
                Some(id) => run::read_info(&results_dir().join(id))?,
                None => {
                    let environment = Environment::capture(&repository(), &workspace);
                    RunInfo {
                        id: new_id(&environment),
                        label,
                        started_at: run::now(),
                        timeout_seconds: timeout_minutes * 60,
                        scenarios: plan(quick, group, samples),
                        environment,
                    }
                }
            };
            run::check_machine(&info.environment);
            let paths = Paths {
                repository: repository(),
                results: results_dir().join(&info.id),
                work: repository().join("target/bench").join(&info.id),
                home,
            };
            run::write_info(&paths.results, &info)?;
            println!("run {}: {} scenarios", info.id, info.scenarios.len());
            run::take(&info, &paths)
        }
        Command::Report { dir } => report::write(&dir),
        Command::List { quick } => {
            let all = plan(quick, Vec::new(), None);
            for scenario in &all {
                println!("{:>3} samples  {}", scenario.samples, scenario.id);
            }
            let total: u32 = all.iter().map(|each| each.samples + 1).sum();
            println!("{} scenarios, {total} processes with warmups", all.len());
            Ok(())
        }
        Command::Sample { work, task } => {
            let task: Task = serde_json::from_str(&task).context("parsing the task")?;
            let lab = Lab {
                workspace: Workspace::new(&home),
                work,
            };
            println!(
                "{}",
                serde_json::to_string(&workload::execute(&lab, &task)?)?
            );
            Ok(())
        }
        Command::Probe {
            work,
            verify,
            trace,
            proof_out,
            case,
        } => {
            let case = match case {
                Some(case) => serde_json::from_str(&case).context("parsing the case")?,
                None => suite::Case::reference(),
            };
            let lab = Lab {
                workspace: Workspace::new(&home),
                work,
            };
            probe::run(
                &lab,
                &case,
                &probe::Options {
                    verify,
                    trace,
                    proof_out,
                },
            )
        }
    }
}
