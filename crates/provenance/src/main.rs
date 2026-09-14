use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use provenance::{
    attack::{self, Outcome},
    demo::{self, Event, DEMO_PLACE},
    manifest,
    verify::{Check, Verdict},
    Workspace,
};
use serde::Serialize;

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
        spinner: RefCell::new(None),
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
    let workspace = Workspace::new(cli.home);
    match cli.command {
        Command::Setup => {
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            workspace.setup(&source, &mut |step| ui.step(step))?;
            ui.finish_spinner();
            ui.line(format!("{} ready", style("✓").green()));
        }
        Command::Demo {
            photo,
            at,
            cell,
            out,
        } => {
            let photo = photo.unwrap_or_else(|| workspace.sample_photo());
            let (at, cell, place) = match (at, cell) {
                (Some(at), Some(cell)) => (at, cell, "the given coordinate"),
                _ => (
                    (DEMO_PLACE.latitude, DEMO_PLACE.longitude),
                    DEMO_PLACE.cell.to_string(),
                    DEMO_PLACE.name,
                ),
            };
            ui.line(format!(
                "{} {} at {place}, cell {cell}",
                style("demo").bold(),
                photo.display()
            ));
            demo::run(&workspace, &photo, at, &cell, &out, &mut |event| {
                ui.stage(event)
            })?;
            ui.line(format!(
                "\npublished {}",
                style(out.join("signed.png").display()).bold()
            ));
            ui.note("the coordinate is a test input, and location proofs hold for an honest prover only (README)");
        }
        Command::Verify { file, cell } => {
            let verdict = workspace.verifier()?.verify(&file, cell.as_deref());
            ui.verdict(&verdict);
            if let Some(step) = &verdict.rejected {
                return Ok(ExitCode::from(step.check.exit_code()));
            }
        }
        Command::Attack { run, only } => {
            let outcomes = attack::run(&workspace, &run, only.as_deref(), &mut |outcome| {
                ui.attack(outcome)
            })?;
            let unexpected = outcomes
                .iter()
                .filter(|outcome| !outcome.as_expected())
                .count();
            if unexpected > 0 {
                ui.line(
                    style(format!(
                        "{unexpected} of {} attacks were not rejected as expected",
                        outcomes.len()
                    ))
                    .red(),
                );
                return Ok(ExitCode::FAILURE);
            }
            ui.line(format!(
                "{} all {} attacks rejected by the expected check",
                style("✓").green(),
                outcomes.len()
            ));
        }
        Command::Inspect { file } => {
            println!("{}", serde_json::to_string_pretty(&manifest::read(&file)?)?)
        }
    }
    Ok(ExitCode::SUCCESS)
}

struct Ui {
    json: bool,
    spinner: RefCell<Option<ProgressBar>>,
}

impl Ui {
    fn emit(&self, value: &impl Serialize) {
        println!(
            "{}",
            serde_json::to_string(value).expect("events serialize")
        );
    }

    fn line(&self, text: impl std::fmt::Display) {
        if !self.json {
            println!("{text}");
        }
    }

    fn note(&self, text: &str) {
        self.line(style(text).dim());
    }

    fn start_spinner(&self, message: String) {
        let spinner = ProgressBar::new_spinner()
            .with_style(
                ProgressStyle::with_template("  {spinner:.cyan} {msg} {elapsed:.dim}")
                    .expect("valid template"),
            )
            .with_message(message);
        spinner.enable_steady_tick(Duration::from_millis(100));
        self.spinner.replace(Some(spinner));
    }

    fn finish_spinner(&self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.finish_and_clear();
        }
    }

    fn step(&self, text: &str) {
        match self.json {
            true => self.emit(&serde_json::json!({"event": "setup", "step": text})),
            false => {
                self.finish_spinner();
                self.start_spinner(text.to_string());
            }
        }
    }

    fn stage(&self, event: Event) {
        if self.json {
            return self.emit(&event);
        }
        match event {
            Event::Started { stage } => self.start_spinner(format!("{:<14}", stage.title())),
            Event::Finished {
                stage,
                detail,
                seconds,
            } => {
                self.finish_spinner();
                println!(
                    "  {} {:<14} {detail} {}",
                    style("✓").green(),
                    stage.title(),
                    style(format!("{seconds:.1}s")).dim()
                );
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
                "\n{}",
                style(format!("rejected by the {}", step.check.title())).red()
            ),
            None => println!("\n{} accepted", style("✓").green()),
        }
    }

    fn attack(&self, outcome: &Outcome) {
        if self.json {
            return self.emit(outcome);
        }
        let summary = attack::ATTACKS
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
                    "rejected by the {}, expected the {}",
                    step.check.title(),
                    outcome.expected.title()
                ),
            ),
            None => (style("✗").red(), "accepted".to_string()),
        };
        println!("  {mark} {:<22} {summary}: {result}", outcome.attack);
    }

    fn error(&self, error: &anyhow::Error) {
        self.finish_spinner();
        match self.json {
            true => {
                self.emit(&serde_json::json!({"event": "error", "message": format!("{error:#}")}))
            }
            false => eprintln!("{} {error:#}", style("error").red().bold()),
        }
    }
}
