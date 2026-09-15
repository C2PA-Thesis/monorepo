use std::{
    cell::RefCell,
    fmt::Display,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{Duration, Instant},
};

use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use provenance::{
    attack::{self, Outcome, ATTACKS},
    demo::{self, Event, Stage, DEMO_PLACE},
    manifest,
    verify::{Check, Verdict},
    Step, Workspace,
};
use serde::Serialize;

const STAGES: [Stage; 6] = [
    Stage::Capture,
    Stage::Crop,
    Stage::LocationProof,
    Stage::CropProof,
    Stage::Publish,
    Stage::Verify,
];

#[derive(Parser)]
#[command(
    version,
    about = "Publish a photo with a crop proof and a location proof in its C2PA manifest, and verify it"
)]
struct Cli {
    /// Where the location prover, proof parameters and keys live.
    #[arg(
        long,
        global = true,
        env = "PROVENANCE_HOME",
        default_value = ".provenance"
    )]
    home: PathBuf,
    /// Print one JSON object per line.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build location-proof, create proof parameters and a device key, download samples.
    Setup,
    /// Capture a photo, publish its left half with both proofs, and verify it.
    Demo {
        /// Photo to capture. Defaults to the C2PA SDK sample.
        #[arg(long)]
        photo: Option<PathBuf>,
        /// Simulated capture coordinate, as LAT,LON. Defaults to UTDT, Buenos Aires.
        #[arg(long, value_parser = parse_coordinate, allow_hyphen_values = true, requires = "cell")]
        at: Option<(f64, f64)>,
        /// H3 cell to prove the coordinate is in.
        #[arg(long, requires = "at")]
        cell: Option<String>,
        /// Directory for the run. `original.png` and `secrets.json` in it are private.
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },
    /// Verify a published PNG as a reader.
    Verify {
        file: PathBuf,
        /// Also require the file to claim this H3 cell.
        #[arg(long)]
        cell: Option<String>,
    },
    /// Forge tampered copies of a demo run; each must be rejected by the expected check.
    Attack {
        /// Directory of the demo run to tamper with.
        #[arg(long, default_value = "out")]
        run: PathBuf,
        /// Run only this attack.
        #[arg(long)]
        only: Option<String>,
    },
    /// Print the provenance assertion of a signed PNG.
    Inspect { file: PathBuf },
}

fn parse_coordinate(text: &str) -> Result<(f64, f64), String> {
    let (latitude, longitude) = text.split_once(',').ok_or("expected LAT,LON")?;
    let parse = |value: &str| {
        value
            .trim()
            .parse::<f64>()
            .map_err(|error| error.to_string())
    };
    Ok((parse(latitude)?, parse(longitude)?))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let ui = Ui {
        json: cli.json,
        running: RefCell::new(None),
    };
    match run(cli, &ui) {
        Ok(code) => code,
        Err(error) => {
            ui.error(&error);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli, ui: &Ui) -> Result<ExitCode> {
    let workspace = Workspace::new(&cli.home);
    match cli.command {
        Command::Setup => {
            ui.header("setup", &[("home", shown(&cli.home))]);
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            workspace.setup(&source, &mut |name, step| ui.setup_step(name, step))?;
            ui.line(format!(
                "\n{} ready. Next: {}",
                style("✓").green(),
                style("provenance demo").bold()
            ));
        }
        Command::Demo {
            photo,
            at,
            cell,
            out,
        } => {
            let photo = photo.unwrap_or_else(|| workspace.sample_photo());
            let (at, cell, place) = match (at, cell) {
                (Some((latitude, longitude)), Some(cell)) => (
                    (latitude, longitude),
                    cell,
                    format!("{latitude}, {longitude}"),
                ),
                _ => (
                    (DEMO_PLACE.latitude, DEMO_PLACE.longitude),
                    DEMO_PLACE.cell.to_string(),
                    format!(
                        "{} ({}, {})",
                        DEMO_PLACE.name, DEMO_PLACE.latitude, DEMO_PLACE.longitude
                    ),
                ),
            };
            ui.header(
                "demo",
                &[
                    ("photo", shown(&photo)),
                    ("place", place),
                    ("cell", cell.clone()),
                    ("out", shown(&out)),
                ],
            );
            let started = Instant::now();
            demo::run(&workspace, &photo, at, &cell, &out, &mut |event| {
                ui.stage(event)
            })?;
            let signed = shown(&out.join("signed.png"));
            ui.line(format!(
                "\n{} published {} in {:.0}s",
                style("✓").green(),
                style(&signed).bold(),
                started.elapsed().as_secs_f64()
            ));
            ui.line(format!(
                "  next  provenance verify {signed}\n        provenance attack --run {}",
                shown(&out)
            ));
            ui.note("\nThe coordinate is a test input. Location proofs hold for an honest prover only; see Known limits in the README.");
        }
        Command::Verify { file, cell } => {
            ensure!(file.exists(), "{} does not exist", file.display());
            ui.header("verify", &[("file", shown(&file))]);
            let verdict = workspace.verifier()?.verify(&file, cell.as_deref());
            ui.verdict(&verdict);
            if let Some(step) = &verdict.rejected {
                return Ok(ExitCode::from(step.check.exit_code()));
            }
        }
        Command::Attack { run, only } => {
            ui.header("attack", &[("run", shown(&run))]);
            let outcomes = attack::run(&workspace, &run, only.as_deref(), &mut |outcome| {
                ui.attack(outcome)
            })?;
            let rejected = outcomes
                .iter()
                .filter(|outcome| outcome.as_expected())
                .count();
            let summary = format!(
                "{rejected} of {} rejected by the expected check",
                plural(outcomes.len(), "attack")
            );
            if rejected < outcomes.len() {
                ui.line(format!("\n{} {summary}", style("✗").red()));
                return Ok(ExitCode::FAILURE);
            }
            ui.line(format!("\n{} {summary}", style("✓").green()));
        }
        Command::Inspect { file } => {
            println!("{}", serde_json::to_string_pretty(&manifest::read(&file)?)?)
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Paths under the working directory are shown relative to it.
fn shown(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn plural(count: usize, noun: &str) -> String {
    match count {
        1 => format!("{count} {noun}"),
        _ => format!("{count} {noun}s"),
    }
}

struct Ui {
    json: bool,
    running: RefCell<Option<(ProgressBar, Instant)>>,
}

impl Ui {
    fn emit(&self, value: &impl Serialize) {
        println!(
            "{}",
            serde_json::to_string(value).expect("events serialize")
        );
    }

    fn line(&self, text: impl Display) {
        if !self.json {
            println!("{text}");
        }
    }

    fn note(&self, text: &str) {
        self.line(style(text).dim());
    }

    fn header(&self, command: &str, fields: &[(&str, String)]) {
        self.line(style(format!("provenance {command}")).bold());
        for (label, value) in fields {
            self.line(format!("  {}  {value}", style(format!("{label:<5}")).dim()));
        }
        self.line("");
    }

    fn start(&self, message: String) {
        let spinner = ProgressBar::new_spinner()
            .with_style(
                ProgressStyle::with_template("  {spinner:.cyan} {msg} {elapsed:.dim}")
                    .expect("valid template"),
            )
            .with_message(message);
        spinner.enable_steady_tick(Duration::from_millis(100));
        self.running.replace(Some((spinner, Instant::now())));
    }

    /// Clears the spinner and returns how long it ran.
    fn stop(&self) -> Duration {
        match self.running.take() {
            Some((spinner, started)) => {
                spinner.finish_and_clear();
                started.elapsed()
            }
            None => Duration::ZERO,
        }
    }

    fn done(&self, label: &str, seconds: f64, detail: &str) {
        println!(
            "  {} {label}  {}  {detail}",
            style("✓").green(),
            style(format!("{seconds:>5.1}s")).dim()
        );
    }

    fn setup_step(&self, name: &str, step: Step) {
        if self.json {
            return self.emit(&serde_json::json!({"event": "setup", "step": name, "state": step}));
        }
        match step {
            Step::Running => self.start(format!("{name:<34}")),
            Step::Done => {
                let seconds = self.stop().as_secs_f64();
                self.done(&format!("{name:<34}"), seconds, "");
            }
            Step::AlreadyPresent => println!(
                "  {} {name:<34}  {}",
                style("·").dim(),
                style("already present").dim()
            ),
        }
    }

    fn stage(&self, event: Event) {
        if self.json {
            return self.emit(&event);
        }
        let label = |stage: Stage| {
            let number = STAGES.iter().position(|each| *each == stage).unwrap_or(0) + 1;
            format!("{number}/{} {:<14}", STAGES.len(), stage.title())
        };
        match event {
            Event::Started { stage } => self.start(label(stage)),
            Event::Finished {
                stage,
                detail,
                seconds,
            } => {
                self.stop();
                self.done(&label(stage), seconds, &detail);
            }
        }
    }

    fn verdict(&self, verdict: &Verdict) {
        if self.json {
            return self.emit(verdict);
        }
        for check in Check::ALL {
            let passed = verdict.passed.iter().find(|step| step.check == check);
            let rejected = verdict.rejected.as_ref().filter(|step| step.check == check);
            let (mark, detail) = match (passed, rejected) {
                (Some(step), _) => (style("✓").green(), step.detail.as_str()),
                (_, Some(step)) => (style("✗").red(), step.detail.as_str()),
                _ => (style("·").dim(), "not run"),
            };
            println!("  {mark} {:<15} {detail}", check.title());
        }
        match &verdict.rejected {
            Some(step) => println!(
                "\n{} rejected by the {} (exit {})",
                style("✗").red(),
                step.check.title(),
                step.check.exit_code()
            ),
            None => {
                println!("\n{} accepted", style("✓").green());
                if let Some(claim) = &verdict.claim {
                    println!(
                        "  these pixels are the left half of an original that device {} signed,\n  together with a coordinate in cell {}, at {} (device time)",
                        provenance::capture::short(&claim.device),
                        claim.cell,
                        claim.captured_at
                    );
                    println!(
                        "{}",
                        style("  The cell holds for an honest location prover only; see Known limits in the README.").dim()
                    );
                }
            }
        }
    }

    fn attack(&self, outcome: &Outcome) {
        if self.json {
            return self.emit(outcome);
        }
        let summary = ATTACKS
            .iter()
            .find(|attack| attack.name == outcome.attack)
            .map_or("", |attack| attack.summary);
        let (mark, result) = match &outcome.verdict.rejected {
            Some(step) if outcome.as_expected() => (
                style("✓").green(),
                format!("rejected by the {}", step.check.title()),
            ),
            Some(step) => (
                style("✗").red(),
                format!(
                    "rejected by the {}, not the {}",
                    step.check.title(),
                    outcome.expected.title()
                ),
            ),
            None => (style("✗").red(), "accepted".to_string()),
        };
        println!(
            "  {mark} {:<21} {result:<31} {}",
            outcome.attack,
            style(summary).dim()
        );
    }

    fn error(&self, error: &anyhow::Error) {
        self.stop();
        match self.json {
            true => {
                self.emit(&serde_json::json!({"event": "error", "message": format!("{error:#}")}))
            }
            false => eprintln!("{} {error:#}", style("error").red().bold()),
        }
    }
}
