use std::{
    cell::RefCell,
    fmt::Display,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{ensure, Result};
use clap::{Parser, Subcommand};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use provenance::{
    attack::{self, Expected, Outcome, ATTACKS},
    capture::{Capture, Coordinate},
    demo::{self, DEMO_PLACE},
    devices::Device,
    manifest, publish,
    publish::{describe_edit, Event, Stage},
    receipt::short,
    serve::{self, Lan, Phase, Server, Status, TLS_PORT},
    verify::{self, Check, Verdict},
    Step, Workspace,
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
    /// Build location-proof, create proof parameters, a device key and the app's TLS key, download samples.
    Setup {
        /// Install this location-proof binary instead of building it with Go.
        #[arg(long, value_name = "BIN")]
        location_proof: Option<PathBuf>,
    },
    /// Capture a photo with the laptop's device key, publish its left half with both proofs, and verify it.
    Demo {
        /// Quarter turns clockwise applied to the published half, 0 to 3.
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..4))]
        turn: u8,
        /// Photo to capture. Defaults to the C2PA SDK sample.
        #[arg(long)]
        photo: Option<PathBuf>,
        /// Simulated capture coordinate, as LAT,LON. Defaults to UTDT, Buenos Aires.
        #[arg(long, value_parser = parse_coordinate, allow_hyphen_values = true, requires = "cell")]
        at: Option<Coordinate>,
        /// H3 cell to prove the coordinate is in.
        #[arg(long, requires = "at")]
        cell: Option<String>,
        /// Directory for the run. `original.png` and `secrets.json` in it are private.
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },
    /// Prove and C2PA-sign a capture directory, from the demo or from a phone.
    Publish {
        /// The capture directory.
        #[arg(long)]
        capture: PathBuf,
        /// Where `crop.png` and `signed.png` go. Defaults to the capture directory.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Serve the capture API, and the capture page when given, for a phone to use.
    Serve {
        /// Port on 127.0.0.1, for this laptop and the tunnel.
        #[arg(long, default_value_t = 8791)]
        port: u16,
        /// Port on every address, with TLS, for the app on the local network.
        #[arg(long, default_value_t = TLS_PORT)]
        tls_port: u16,
        /// Serve only on 127.0.0.1, without the app's TLS listener.
        #[arg(long)]
        no_tls: bool,
        /// Another https address for the app's QR code, tried first, such as a
        /// public IP no interface carries. Repeatable.
        #[arg(long = "app-url", value_name = "URL", conflicts_with = "no_tls")]
        app_urls: Vec<String>,
        /// Where uploaded captures land, one directory each.
        #[arg(long, default_value = "captures")]
        captures: PathBuf,
        /// Built pages to serve: the capture page at `/`, the presenter page at `/screen`.
        #[arg(long)]
        web: Option<PathBuf>,
        /// The tunnel's https address, for the QR code on the presenter page.
        #[arg(long, env = "PUBLIC_URL")]
        public_url: Option<String>,
        /// Store uploads without publishing them.
        #[arg(long)]
        no_publish: bool,
        /// The app's App ID (Team ID, a period, the bundle id), to check App
        /// Attest attestations against. Without it they are not checked.
        #[arg(long, env = "PROVENANCE_APP_ID")]
        app_id: Option<String>,
        /// Refuse a secure-enclave pairing that carries no App Attest attestation.
        #[arg(long, requires = "app_id")]
        require_attestation: bool,
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
    /// List the trusted device keys, or revoke one.
    Devices {
        #[command(subcommand)]
        action: Option<DevicesAction>,
    },
}

#[derive(Subcommand)]
enum DevicesAction {
    /// Stop trusting a device key. Files it signed no longer verify here.
    Revoke {
        /// The device id, or enough of its first hex digits to name one key.
        id: String,
    },
}

fn parse_coordinate(text: &str) -> Result<Coordinate, String> {
    let (latitude, longitude) = text.split_once(',').ok_or("expected LAT,LON")?;
    let parse = |value: &str| {
        value
            .trim()
            .parse::<f64>()
            .map_err(|error| error.to_string())
    };
    Ok(Coordinate {
        latitude: parse(latitude)?,
        longitude: parse(longitude)?,
    })
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
        Command::Setup { location_proof } => {
            ui.header("setup", &[("home", shown(&cli.home))]);
            let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            workspace.setup(&source, location_proof.as_deref(), &mut |name, step| {
                ui.setup_step(name, step)
            })?;
            ui.line(format!(
                "\n{} ready. Next: {}",
                style("✓").green(),
                style("provenance demo").bold()
            ));
        }
        Command::Demo {
            turn,
            photo,
            at,
            cell,
            out,
        } => {
            let photo = photo.unwrap_or_else(|| workspace.sample_photo());
            let (coordinate, cell, place) = match (at, cell) {
                (Some(coordinate), Some(cell)) => (
                    coordinate,
                    cell,
                    format!("{}, {}", coordinate.latitude, coordinate.longitude),
                ),
                _ => (
                    DEMO_PLACE.coordinate,
                    DEMO_PLACE.cell.to_string(),
                    format!(
                        "{} ({}, {})",
                        DEMO_PLACE.name,
                        DEMO_PLACE.coordinate.latitude,
                        DEMO_PLACE.coordinate.longitude
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
            let turn = crop_proof::Turn::new(turn)?;
            demo::run(
                &workspace,
                &photo,
                coordinate,
                &cell,
                turn,
                &out,
                &mut |event| ui.stage(event, &demo::STAGES),
            )?;
            ui.published(&out, started);
            ui.note("\nThe coordinate is a test input. The cell holds if the signing device computed the trigonometric hints honestly; see Known limits in the README.");
        }
        Command::Publish { capture, out } => {
            let out = out.unwrap_or_else(|| capture.clone());
            ui.header(
                "publish",
                &[("capture", shown(&capture)), ("out", shown(&out))],
            );
            let started = Instant::now();
            let capture = Capture::read(&capture)?;
            publish::run(&workspace, &capture, &out, &mut |event| {
                ui.stage(event, &publish::STAGES)
            })?;
            ui.published(&out, started);
        }
        Command::Serve {
            port,
            tls_port,
            no_tls,
            app_urls: extra_app_urls,
            captures,
            web,
            public_url,
            no_publish,
            app_id,
            require_attestation,
        } => {
            let tls = match no_tls {
                true => None,
                false => Some(workspace.tls().load()?),
            };
            let mut server = Server::new(workspace, captures.clone())
                .public_url(public_url)
                .app_attest(app_id.clone(), require_attestation)?
                .on_change(serve_listener(ui.json));
            let app_urls = match &tls {
                Some(tls) => serve::app_urls(&tls.addresses, tls_port, &extra_app_urls)?,
                None => Vec::new(),
            };
            if let Some(tls) = &tls {
                server = server.lan(Lan {
                    urls: app_urls.clone(),
                    pin: tls.pin.clone(),
                });
            }
            if !no_publish {
                server = server.publishing();
            }
            let url = format!("http://127.0.0.1:{port}");
            if ui.json {
                ui.emit(&serde_json::json!({
                    "event": "serving", "url": url, "captures": captures,
                    "pairing_code": server.pairing_code(), "pair_url": server.pair_url(),
                    "app_urls": app_urls, "pin": tls.as_ref().map(|tls| &tls.pin),
                    "app_id": app_id, "require_attestation": require_attestation,
                }));
            } else {
                let mut fields = vec![
                    ("url", url.clone()),
                    ("captures", shown(&captures)),
                    ("pair", server.pairing_code().to_string()),
                ];
                if let Some(tls) = &tls {
                    let app = match app_urls.is_empty() {
                        true => format!("port {tls_port}, but no network address yet"),
                        false => app_urls.join("  "),
                    };
                    fields.insert(1, ("app", app));
                    fields.insert(2, ("pin", short(&tls.pin)));
                }
                if web.is_some() {
                    fields.insert(1, ("screen", format!("{url}/screen")));
                }
                fields.push((
                    "app attest",
                    match (&app_id, require_attestation) {
                        (None, _) => "not checked (no --app-id)".to_string(),
                        (Some(app_id), false) => app_id.clone(),
                        (Some(app_id), true) => format!("{app_id}, required"),
                    },
                ));
                if let Some(pair_url) = server.pair_url() {
                    fields.insert(0, ("phone", pair_url));
                }
                ui.header("serve", &fields);
                ui.note(match no_publish {
                    false => "Each upload is proved, signed and verified here as soon as it arrives. Ctrl-C stops.\n",
                    true => "Uploads are stored only; publish one with `provenance publish --capture DIR`. Ctrl-C stops.\n",
                });
            }
            let tls = tls.map(|tls| (tls.config, tls_port));
            serve::run(Arc::new(server).router(web), port, tls)?;
        }
        Command::Verify { file, cell } => {
            ensure!(file.exists(), "{} does not exist", file.display());
            ui.header("verify", &[("file", shown(&file))]);
            let verdict = workspace
                .verifier()?
                .verify(&file, cell.as_deref(), &mut |event| ui.check(event));
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
            let as_expected = outcomes
                .iter()
                .filter(|outcome| outcome.as_expected())
                .count();
            let summary = format!(
                "{as_expected} of {} ended as expected",
                plural(outcomes.len(), "attack")
            );
            if as_expected < outcomes.len() {
                ui.line(format!("\n{} {summary}", style("✗").red()));
                return Ok(ExitCode::FAILURE);
            }
            ui.line(format!("\n{} {summary}", style("✓").green()));
        }
        Command::Inspect { file } => {
            println!("{}", serde_json::to_string_pretty(&manifest::read(&file)?)?)
        }
        Command::Devices { action: None } => {
            let devices = workspace.devices().list()?;
            if ui.json {
                devices.iter().for_each(|device| ui.emit(device));
            } else {
                ui.header(
                    "devices",
                    &[("trusted", shown(&workspace.trusted_keys_dir()))],
                );
                let laptop = workspace.device_key().and_then(|key| key.id()).ok();
                for device in &devices {
                    ui.device(device, laptop.as_deref() == Some(device.id.as_str()));
                }
                ui.line(format!("\n{}", plural(devices.len(), "trusted key")));
            }
        }
        Command::Devices {
            action: Some(DevicesAction::Revoke { id }),
        } => {
            let device = workspace.devices().revoke(&id)?;
            match ui.json {
                true => ui.emit(&serde_json::json!({"event": "revoked", "device": device})),
                false => {
                    ui.device(&device, false);
                    ui.line(format!(
                        "\n{} revoked {}",
                        style("✓").green(),
                        short(&device.id)
                    ));
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// One line on the laptop's terminal each time a capture changes phase.
fn serve_listener(json: bool) -> impl Fn(&Status) + Send + Sync + 'static {
    move |status| {
        if json {
            let event = serde_json::json!({"event": "capture", "id": status.id, "phase": status.phase, "error": status.error});
            return println!("{event}");
        }
        let id = style(&status.id).bold();
        match status.phase {
            Phase::Queued => println!(
                "  {} {id}  from {} device {}, cell {}, crop {}",
                style("↓").cyan(),
                status.device_kind.map_or("a", |kind| kind.title()),
                short(&status.device),
                status.capture.cell,
                describe_edit(status.capture.crop, status.capture.turn)
            ),
            Phase::Publishing => println!("  {} {id}  proving", style("…").dim()),
            Phase::Accepted => println!("  {} {id}  published and verified", style("✓").green()),
            Phase::Rejected | Phase::Failed => println!(
                "  {} {id}  {}",
                style("✗").red(),
                style(status.error.as_deref().unwrap_or("failed")).red()
            ),
            Phase::Stored | Phase::Published | Phase::Verifying => {}
        }
    }
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
            self.line(format!("  {}  {value}", style(format!("{label:<8}")).dim()));
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

    /// `stages` numbers the stage among those the command runs.
    fn stage(&self, event: Event, stages: &[Stage]) {
        if self.json {
            return self.emit(&event);
        }
        let label = |stage: Stage| {
            let number = stages.iter().position(|each| *each == stage).unwrap_or(0) + 1;
            format!("{number}/{} {:<14}", stages.len(), stage.title())
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

    fn published(&self, out: &Path, started: Instant) {
        let signed = shown(&out.join("signed.png"));
        self.line(format!(
            "\n{} published {} in {:.0}s",
            style("✓").green(),
            style(&signed).bold(),
            started.elapsed().as_secs_f64()
        ));
        self.line(format!(
            "  next  provenance verify {signed}\n        provenance attack --run {}",
            shown(out)
        ));
    }

    /// One line per check: what it set out to establish, then how it ended.
    fn check(&self, event: verify::Event) {
        if self.json {
            return self.emit(&event);
        }
        let label = |check: Check| format!("{:<15}", check.title());
        match event {
            verify::Event::Checking { check, intent } => {
                self.start(format!("{} {intent}", label(check)))
            }
            verify::Event::Passed {
                check,
                intent,
                outcome,
                seconds,
            } => {
                self.stop();
                self.done(
                    &label(check),
                    seconds,
                    &format!("{intent} → {}", style(outcome).green()),
                );
            }
            verify::Event::Rejected {
                check,
                intent,
                reason,
                seconds,
            } => {
                self.stop();
                println!(
                    "  {} {}  {}  {intent} → {}",
                    style("✗").red(),
                    label(check),
                    style(format!("{seconds:>5.1}s")).dim(),
                    style(reason).red()
                );
            }
        }
    }

    fn verdict(&self, verdict: &Verdict) {
        if self.json {
            return self.emit(verdict);
        }
        let ran = |check: Check| {
            verdict.passed.iter().any(|step| step.check == check)
                || verdict
                    .rejected
                    .as_ref()
                    .is_some_and(|step| step.check == check)
        };
        for check in Check::ALL.into_iter().filter(|check| !ran(*check)) {
            println!(
                "  {} {:<15}  {}",
                style("·").dim(),
                check.title(),
                style("not run").dim()
            );
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
                        "  these pixels are the {} of an original that device {} signed,\n  together with an envelope whose sealed hints map to cell {}, at {} (device time)",
                        describe_edit(claim.crop, claim.turn),
                        short(&claim.device),
                        claim.cell,
                        claim.captured_at
                    );
                    println!(
                        "{}",
                        style("  The sealed coordinate lies in that cell if the signing device computed the hints from it; see Known limits in the README.").dim()
                    );
                }
            }
        }
    }

    /// `laptop` names this laptop's own key when no sidecar says so.
    fn device(&self, device: &Device, laptop: bool) {
        let (kind, name, paired_at) = match &device.info {
            Some(info) => (info.shown(), info.name.as_str(), info.paired_at.as_str()),
            None if laptop => ("laptop".to_string(), "this laptop", ""),
            None => (
                "unknown".to_string(),
                "",
                "paired before kinds were recorded",
            ),
        };
        println!(
            "  {}  {kind:<38}  {name:<20}  {}",
            style(&device.id).bold(),
            style(paired_at).dim()
        );
    }

    fn attack(&self, outcome: &Outcome) {
        if self.json {
            return self.emit(outcome);
        }
        let summary = ATTACKS
            .iter()
            .find(|attack| attack.name == outcome.attack)
            .map_or("", |attack| attack.summary);
        let (mark, result) = match (outcome.expected, &outcome.verdict.rejected) {
            (Expected::Rejected(expected), Some(step)) if step.check == expected => (
                style("✓").green(),
                format!("rejected by the {}", step.check.title()),
            ),
            (Expected::Rejected(expected), Some(step)) => (
                style("✗").red(),
                format!(
                    "rejected by the {}, not the {}",
                    step.check.title(),
                    expected.title()
                ),
            ),
            (Expected::Rejected(_), None) => (style("✗").red(), "accepted".to_string()),
            // Accepted by design: the pipeline trusts the device for these.
            (Expected::Accepted, None) => (style("!").yellow(), "accepted, trusted".to_string()),
            (Expected::Accepted, Some(step)) => (
                style("✗").red(),
                format!(
                    "rejected by the {}, expected acceptance",
                    step.check.title()
                ),
            ),
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
