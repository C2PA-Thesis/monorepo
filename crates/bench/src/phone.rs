//! Runs ios/ProvenanceCaptureTests/BenchmarkTests on a paired iPhone. A Simulator
//! would measure this laptop, so without a real phone the scenarios are skipped.

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
    measure::{self, Exit},
    run::tail,
    suite::Op,
};

const TEAM_VARIABLE: &str = "BENCH_IOS_TEAM";
const TEST: &str = "ProvenanceCaptureTests/BenchmarkTests";
const PARAMETERS: [&str; 3] = ["proving.key", "circuit.r1cs", "version"];
const UNLOCK_WAIT: Duration = Duration::from_secs(120);
const UNLOCK_POLL: Duration = Duration::from_secs(5);
const TEST_TIMEOUT: Duration = Duration::from_secs(40 * 60);
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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

fn build_libraries(repository: &Path) -> Result<()> {
    for (framework, script) in [("Fingerprint", "build-rust.sh"), ("ZKLoc", "build-go.sh")] {
        let built = repository.join(format!("ios/Frameworks/{framework}.xcframework"));
        if built.exists() {
            continue;
        }
        let usage = measure::run(
            &mut Command::new(repository.join("ios/scripts").join(script)),
            BUILD_TIMEOUT,
        )?;
        ensure!(
            usage.outcome == Exit::Success,
            "{script}: {}",
            tail(&usage.stderr)
        );
    }
    Ok(())
}

fn copy_parameters(params: &Path, repository: &Path) -> Result<()> {
    let bundled = repository.join("ios/ProvenanceCaptureTests/Fixtures/bench");
    fs::create_dir_all(&bundled)?;
    for name in PARAMETERS {
        fs::copy(params.join(name), bundled.join(name))
            .with_context(|| format!("copying {name}"))?;
    }
    Ok(())
}

pub fn measure(
    repository: &Path,
    params: &Path,
    work: &Path,
    samples: u32,
) -> Result<Vec<Reading>> {
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
    copy_parameters(params, repository)?;

    let usage = measure::run(
        Command::new("xcodebuild")
            .args(["test", "-project", "ProvenanceCapture.xcodeproj"])
            .args(["-scheme", "ProvenanceCapture", "-allowProvisioningUpdates"])
            // A parallel test clone keeps its output, and the BENCH lines with it.
            .args(["-parallel-testing-enabled", "NO"])
            .arg("-destination")
            .arg(format!("id={udid}"))
            .arg(format!("-only-testing:{TEST}"))
            .arg("-derivedDataPath")
            .arg(work.join("ios"))
            .arg(format!("DEVELOPMENT_TEAM={team}"))
            .env("TEST_RUNNER_BENCH_SAMPLES", samples.to_string())
            .current_dir(repository.join("ios")),
        TEST_TIMEOUT,
    )?;
    match usage.outcome {
        Exit::Success => parse(&usage.stdout),
        Exit::TimedOut => bail!("the iPhone test timed out"),
        Exit::Failed(why) => bail!("xcodebuild: {why}: {}", tail(&usage.stdout)),
    }
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
    fn a_passing_test_without_measurements_is_an_error() {
        assert!(parse("** TEST SUCCEEDED **\n").is_err());
    }

    #[test]
    fn a_damaged_line_is_an_error() {
        assert!(parse("BENCH fingerprint 0\n").is_err());
        assert!(parse("BENCH unknown 0 1.0 0 1 0\n").is_err());
    }
}
