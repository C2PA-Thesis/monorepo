//! Runs ios/ProvenanceCaptureTests/BenchmarkTests on a paired iPhone. A Simulator
//! would measure this laptop, so without a real phone the scenarios are skipped.
//!
//! Each test method runs in its own `xcodebuild test`, so in its own process.
//! The phone reports the largest footprint the process ever had, and in one
//! process the crop proof would set that figure for the capture too. A crop
//! proof that iOS kills for its memory also leaves the capture's samples.
//!
//! Everything a test prints is kept in a file of the run's work directory.
//! The crop test prints its memory twice a second, and after a kill those
//! lines are the only record of how far the proof got.

use std::{
    env, fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;

use crate::{
    measure::{self, Exit, Usage},
    run::tail,
    suite::Op,
};

const TEAM_VARIABLE: &str = "BENCH_IOS_TEAM";
const TESTS: [Test; 2] = [
    Test {
        name: "ProvenanceCaptureTests/BenchmarkTests/testMeasureACapture",
        ops: &[
            Op::PhoneFingerprint,
            Op::PhoneEnvelope,
            Op::PhoneLocationProve,
        ],
        parameters: |parameters| parameters.location,
        files: &["proving.key", "circuit.r1cs", "version"],
        folder: "",
        round: Duration::from_secs(2 * 60),
    },
    Test {
        name: "ProvenanceCaptureTests/BenchmarkTests/testMeasureACropProof",
        ops: &[Op::PhoneCropProve],
        parameters: |parameters| parameters.crop,
        files: &["metadata.json", "prover.bin", "verifier.bin"],
        folder: "crop",
        round: Duration::from_secs(6 * 60),
    },
];
const BUNDLED: &str = "ios/ProvenanceCaptureTests/Fixtures/bench";
const UNLOCK_WAIT: Duration = Duration::from_secs(120);
const UNLOCK_POLL: Duration = Duration::from_secs(5);
/// Building the app and the tests and installing them, before any round.
/// The crop parameters make the test bundle 200 MB larger.
const INSTALL_ALLOWANCE: Duration = Duration::from_secs(20 * 60);
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// One test method, the operations it measures and what it needs.
struct Test {
    name: &'static str,
    ops: &'static [Op],
    /// The directory `provenance setup` wrote its parameter files to.
    parameters: for<'a> fn(&Parameters<'a>) -> &'a Path,
    files: &'static [&'static str],
    /// Where the files go under `BUNDLED`. Xcode puts every file under
    /// Fixtures at the root of the test bundle, so a folder only keeps the
    /// sets apart on disk.
    folder: &'static str,
    /// The longest one round may take, with its pause.
    round: Duration,
}

impl Test {
    /// The method's own name, for the file that keeps its output.
    fn method(&self) -> &'static str {
        self.name.rsplit('/').next().unwrap_or(self.name)
    }

    /// Long enough for every round, so a slow phone loses no sample, and
    /// short enough that a hung test does not hold the run for hours.
    fn timeout(&self, samples: u32) -> Duration {
        // Round 0 is the warmup.
        INSTALL_ALLOWANCE + self.round * (samples + 1)
    }

    /// The test bundle must hold the parameters; a test that is not wanted
    /// costs no copy and needs no parameters.
    fn copy_parameters(&self, parameters: &Parameters, repository: &Path) -> Result<()> {
        let from = (self.parameters)(parameters);
        let to = repository.join(BUNDLED).join(self.folder);
        fs::create_dir_all(&to)?;
        for name in self.files {
            fs::copy(from.join(name), to.join(name))
                .with_context(|| format!("copying {name} from {}", from.display()))?;
        }
        Ok(())
    }
}

/// The parameter directories `provenance setup` wrote.
pub struct Parameters<'a> {
    pub location: &'a Path,
    pub crop: &'a Path,
}

/// What one test measured. A test that stops early still gives the rounds
/// it finished, and `failure` says why it stopped.
pub struct Batch {
    pub ops: &'static [Op],
    pub readings: Vec<Reading>,
    pub failure: Option<String>,
}

/// One timed operation, as the test printed it.
#[derive(Debug, PartialEq)]
pub struct Reading {
    pub op: Op,
    pub round: u32,
    pub seconds: f64,
    pub bytes: u64,
    pub peak_footprint_bytes: u64,
    pub thermal_state: u64,
}

#[derive(Deserialize)]
struct Reply<T> {
    result: T,
}

#[derive(Deserialize)]
struct DeviceList {
    devices: Vec<Device>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    connection_properties: ConnectionProperties,
    device_properties: DeviceProperties,
    hardware_properties: HardwareProperties,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionProperties {
    pairing_state: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceProperties {
    os_version_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HardwareProperties {
    udid: Option<String>,
    marketing_name: Option<String>,
    platform: Option<String>,
    reality: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockState {
    passcode_required: bool,
}

/// Runs `devicectl` with `args` and parses the JSON it writes.
fn devicectl<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T> {
    let file = tempfile::NamedTempFile::new()?;
    let usage = measure::run(
        Command::new("xcrun")
            .arg("devicectl")
            .args(args)
            .arg("--json-output")
            .arg(file.path()),
        TOOL_TIMEOUT,
    )?;
    ensure!(
        usage.outcome == Exit::Success,
        "devicectl failed: {}",
        tail(&usage.stderr)
    );
    let reply: Reply<T> = serde_json::from_slice(&fs::read(file.path())?)?;
    Ok(reply.result)
}

fn paired_iphone() -> Result<Device> {
    let list: DeviceList = devicectl(&["list", "devices"])?;
    list.devices
        .into_iter()
        .find(|device| {
            let hardware = &device.hardware_properties;
            hardware.reality.as_deref() == Some("physical")
                && hardware.platform.as_deref() == Some("iOS")
                && device.connection_properties.pairing_state.as_deref() == Some("paired")
        })
        .context("no paired iPhone")
}

/// The paired iPhone in words, for the run's record.
pub fn describe() -> Option<String> {
    let device = paired_iphone().ok()?;
    let name = device.hardware_properties.marketing_name?;
    let version = device.device_properties.os_version_number?;
    Some(format!("{name}, iOS {version}"))
}

/// devicectl fails now and then on a phone that sleeps, so an error here
/// means "ask again", as a locked phone does.
fn wait_until_unlocked(udid: &str) -> Result<()> {
    let started = Instant::now();
    let mut told = false;
    loop {
        let state = devicectl::<LockState>(&["device", "info", "lockState", "--device", udid]);
        if state.is_ok_and(|state| !state.passcode_required) {
            return Ok(());
        }
        ensure!(started.elapsed() < UNLOCK_WAIT, "the iPhone stayed locked");
        if !told {
            println!("unlock the iPhone to measure it");
            told = true;
        }
        thread::sleep(UNLOCK_POLL);
    }
}

/// The Rust libraries are built on every run. The app links whatever is in
/// ios/Frameworks, and a library from before the last change to the prover
/// would be measured in the name of the new one. Cargo does nothing for
/// sources that did not change. The Go library has no part in the crop
/// proof, so it is built only when it is missing.
fn build_libraries(repository: &Path) -> Result<()> {
    build(repository, "build-rust.sh")?;
    if !repository.join("ios/Frameworks/ZKLoc.xcframework").exists() {
        build(repository, "build-go.sh")?;
    }
    Ok(())
}

fn build(repository: &Path, script: &str) -> Result<()> {
    let usage = measure::run(
        &mut Command::new(repository.join("ios/scripts").join(script)),
        BUILD_TIMEOUT,
    )?;
    ensure!(
        usage.outcome == Exit::Success,
        "{script}: {}",
        tail(&usage.stderr)
    );
    Ok(())
}

/// Measures `ops` on the paired iPhone, one batch for each test that has any
/// of them. An error means that nothing could run.
pub fn measure(
    repository: &Path,
    parameters: &Parameters,
    work: &Path,
    ops: &[Op],
    samples: u32,
) -> Result<Vec<Batch>> {
    let team = env::var(TEAM_VARIABLE)
        .ok()
        .filter(|team| !team.is_empty())
        .with_context(|| format!("set {TEAM_VARIABLE} to your Apple team id"))?;
    let device = paired_iphone()?;
    let udid = device
        .hardware_properties
        .udid
        .context("the iPhone has no UDID")?;
    wait_until_unlocked(&udid)?;
    build_libraries(repository)?;

    Ok(TESTS
        .iter()
        .filter(|test| test.ops.iter().any(|op| ops.contains(op)))
        .map(|test| {
            let (readings, failure) = test
                .copy_parameters(parameters, repository)
                .and_then(|()| xcodebuild(test, repository, &udid, &team, work, samples))
                .map_or_else(
                    |error| (Vec::new(), Some(format!("{error:#}"))),
                    |usage| read(test, &usage, work),
                );
            Batch {
                ops: test.ops,
                readings,
                failure,
            }
        })
        .collect())
}

fn xcodebuild(
    test: &Test,
    repository: &Path,
    udid: &str,
    team: &str,
    work: &Path,
    samples: u32,
) -> Result<Usage> {
    measure::run(
        Command::new("xcodebuild")
            .args(["test", "-project", "ProvenanceCapture.xcodeproj"])
            .args(["-scheme", "ProvenanceCapture", "-allowProvisioningUpdates"])
            // A parallel test clone keeps its output, and the BENCH lines with it.
            .args(["-parallel-testing-enabled", "NO"])
            .arg("-destination")
            .arg(format!("id={udid}"))
            .arg(format!("-only-testing:{}", test.name))
            .arg("-derivedDataPath")
            .arg(work.join("ios"))
            .arg(format!("DEVELOPMENT_TEAM={team}"))
            .env("TEST_RUNNER_BENCH_SAMPLES", samples.to_string())
            .current_dir(repository.join("ios")),
        test.timeout(samples),
    )
}

/// Keeps what the test printed in `work` and reads its measurements. A test
/// that failed or timed out still gives the rounds it finished, with the
/// reason it stopped.
fn read(test: &Test, usage: &Usage, work: &Path) -> (Vec<Reading>, Option<String>) {
    let log = work.join(format!("phone-{}.log", test.method()));
    let kept = match fs::write(&log, &usage.stdout) {
        Ok(()) => format!("its output is in {}", log.display()),
        Err(error) => format!("its output was not kept: {error}"),
    };
    println!("  {}: {kept}", test.method());
    for digest in proof_digests(&usage.stdout) {
        println!("  crop proof made on the iPhone: SHA-256 {digest}");
    }
    let (readings, failure) = match &usage.outcome {
        Exit::Success => match parse(&usage.stdout) {
            Ok(readings) => (readings, None),
            Err(error) => (Vec::new(), Some(format!("{error:#}"))),
        },
        Exit::TimedOut => (
            finished(&usage.stdout),
            Some(format!(
                "the iPhone test timed out after {:.0} min",
                usage.wall_seconds / 60.0
            )),
        ),
        Exit::Failed(why) => (
            finished(&usage.stdout),
            Some(format!("xcodebuild: {why}: {}", tail(&usage.stdout))),
        ),
    };
    (readings, failure.map(|why| format!("{why}; {kept}")))
}

/// The digests in the `CROP-PROOF round bytes sha256` lines, each once. The
/// proof of one image with one set of parameters is always the same bytes,
/// so `crates/crop-proof-ffi/c/check.sh` must print the same digest on the
/// laptop. Another one means the phone ran another prover.
fn proof_digests(output: &str) -> Vec<&str> {
    let mut digests: Vec<&str> = output
        .lines()
        .filter_map(|line| line.strip_prefix("CROP-PROOF "))
        .filter_map(|line| line.split_whitespace().nth(2))
        .collect();
    digests.sort_unstable();
    digests.dedup();
    digests
}

/// The measurements of a test that did not end well. Its last line may be
/// cut, so a line that does not parse is left out.
fn finished(output: &str) -> Vec<Reading> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("BENCH "))
        .filter_map(|line| parse_line(line).ok())
        .collect()
}

/// Lines of `BENCH operation round seconds bytes peak-footprint thermal-state`.
fn parse(output: &str) -> Result<Vec<Reading>> {
    let readings: Vec<Reading> = output
        .lines()
        .filter_map(|line| line.strip_prefix("BENCH "))
        .map(parse_line)
        .collect::<Result<_>>()?;
    ensure!(
        !readings.is_empty(),
        "the iPhone test printed no measurements"
    );
    Ok(readings)
}

fn parse_line(line: &str) -> Result<Reading> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [operation, round, seconds, bytes, peak, thermal] = fields[..] else {
        bail!("expected six fields in {line:?}");
    };
    Ok(Reading {
        op: match operation {
            "fingerprint" => Op::PhoneFingerprint,
            "envelope" => Op::PhoneEnvelope,
            "location-prove" => Op::PhoneLocationProve,
            "crop-prove" => Op::PhoneCropProve,
            other => bail!("unknown operation {other:?}"),
        },
        round: round.parse()?,
        seconds: seconds.parse()?,
        bytes: bytes.parse()?,
        peak_footprint_bytes: peak.parse()?,
        thermal_state: thermal.parse()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::{scenarios, Depth, Group};

    #[test]
    fn measurements_are_read_among_the_build_output() {
        let output = "Test Case started.\n\
                      BENCH fingerprint 0 1.478571958 0 20989744 0\n\
                      BENCH location-prove 1 1.28 196 83183456 1\n\
                      ** TEST SUCCEEDED **\n";
        let readings = parse(output).unwrap();
        assert_eq!(readings.len(), 2);
        assert_eq!(
            readings[1],
            Reading {
                op: Op::PhoneLocationProve,
                round: 1,
                seconds: 1.28,
                bytes: 196,
                peak_footprint_bytes: 83183456,
                thermal_state: 1,
            }
        );
    }

    #[test]
    fn a_crop_proof_is_read_among_its_memory_log() {
        let output = "CROP-MEMORY 1 before 67848904 5873172480\n\
                      CROP-MEMORY 1 0.5 269290328 5671731056\n\
                      CROP-MEMORY 1 after 283609944 5650000000\n\
                      CROP-PROOF 1 36692 0d6ac08f976214a4\n\
                      CROP-MEMORY 1 freed 67900000 5870000000\n\
                      CROP-PHASES 1 load 0.92 prove 39.79 verify 2.45\n\
                      BENCH crop-prove 1 40.712313333000004 36692 1796656920 2\n";
        assert_eq!(proof_digests(output), ["0d6ac08f976214a4"]);
        assert_eq!(
            parse(output).unwrap(),
            [Reading {
                op: Op::PhoneCropProve,
                round: 1,
                seconds: 40.712313333000004,
                bytes: 36692,
                peak_footprint_bytes: 1796656920,
                thermal_state: 2,
            }]
        );
    }

    #[test]
    fn every_phone_operation_belongs_to_one_test() {
        let covered: Vec<Op> = TESTS.iter().flat_map(|test| test.ops).copied().collect();
        for scenario in scenarios(&Group::ALL, Depth::QUICK) {
            let tests = covered.iter().filter(|op| **op == scenario.op).count();
            assert_eq!(
                tests,
                usize::from(scenario.op.is_phone()),
                "{:?}",
                scenario.op
            );
        }
    }

    fn usage(outcome: Exit, stdout: &str) -> Usage {
        Usage {
            wall_seconds: 600.0,
            user_seconds: 0.0,
            system_seconds: 0.0,
            peak_rss_bytes: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
            outcome,
        }
    }

    /// iOS kills a proof that takes too much memory. The rounds before it
    /// are measurements, and the memory log up to the kill is on disk.
    #[test]
    fn a_killed_test_keeps_its_finished_rounds_and_its_output() {
        let crop = &TESTS[1];
        let output = "BENCH crop-prove 0 40.7 36692 1796656920 0\n\
                      CROP-PROOF 0 36692 aa11\n\
                      BENCH crop-prove 1 41.2 36692 1796656920 1\n\
                      CROP-PROOF 1 36692 aa11\n\
                      CROP-MEMORY 2 12.5 2900000000 90000000\n\
                      BENCH crop-prove 2 39.";
        let work = tempfile::tempdir().unwrap();
        let log = work.path().join("phone-testMeasureACropProof.log");
        for outcome in [Exit::Failed("exit status 65".to_string()), Exit::TimedOut] {
            let (readings, failure) = read(crop, &usage(outcome, output), work.path());
            let rounds: Vec<u32> = readings.iter().map(|reading| reading.round).collect();
            assert_eq!(rounds, [0, 1]);
            let failure = failure.unwrap();
            assert!(failure.contains(&log.display().to_string()), "{failure}");
            assert_eq!(fs::read_to_string(&log).unwrap(), output);
        }
        assert_eq!(proof_digests(output), ["aa11"]);
    }

    #[test]
    fn a_passing_test_reads_strictly() {
        let work = tempfile::tempdir().unwrap();
        let passed = |output| read(&TESTS[0], &usage(Exit::Success, output), work.path());

        let (readings, failure) = passed("BENCH envelope 0 0.01 0 20989744 0\n");
        assert_eq!((readings.len(), failure), (1, None));
        let (readings, failure) = passed("BENCH envelope 0 0.01 0 20989744 0\nBENCH envelope 1\n");
        assert!(readings.is_empty());
        assert!(failure.unwrap().contains("expected six fields"));
        let (_, failure) = passed("** TEST SUCCEEDED **\n");
        assert!(failure.unwrap().contains("no measurements"));
    }

    #[test]
    fn the_timeout_grows_with_the_samples() {
        let [capture, crop] = &TESTS;
        assert_eq!(crop.timeout(0), INSTALL_ALLOWANCE + crop.round);
        assert_eq!(crop.timeout(10) - crop.timeout(9), crop.round);
        assert!(capture.timeout(10) < crop.timeout(10));
    }

    #[test]
    fn a_test_copies_only_its_own_parameters() {
        let home = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        // Only the crop parameters exist, as if the location setup was missing.
        let crop = home.path().join("crop");
        fs::create_dir(&crop).unwrap();
        for name in TESTS[1].files {
            fs::write(crop.join(name), name).unwrap();
        }
        let parameters = Parameters {
            location: &home.path().join("location"),
            crop: &crop,
        };

        TESTS[1]
            .copy_parameters(&parameters, repository.path())
            .unwrap();
        let bundled = repository.path().join(BUNDLED);
        assert_eq!(
            fs::read_to_string(bundled.join("crop/prover.bin")).unwrap(),
            "prover.bin"
        );
        assert!(!bundled.join("proving.key").exists());
        let missing = TESTS[0].copy_parameters(&parameters, repository.path());
        assert!(format!("{:#}", missing.unwrap_err()).contains("proving.key"));
    }

    #[test]
    fn a_passing_test_without_measurements_is_an_error() {
        assert!(parse("** TEST SUCCEEDED **\n").is_err());
    }

    #[test]
    fn a_damaged_line_is_an_error() {
        assert!(parse("BENCH fingerprint 0\n").is_err());
        assert!(parse("BENCH unknown 0 1.0 0 1 0\n").is_err());
    }
}
