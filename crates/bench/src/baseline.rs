//! Upstream code on this machine: HyperVerITAS and ZKLP as published, and the
//! fingerprint the capture page runs. Each builds into the run's scratch
//! space. A baseline that cannot build is skipped; it never stops the run.

use std::{
    env, fs, iter,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{bail, ensure, Context, Result};
use crop_proof::{RgbImage, Size};
use provenance::image;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    measure::{self, Exit},
    suite::{Case, Op},
    workload::{Lab, Measured},
};

const HYPERVERITAS_URL: &str = "https://github.com/glgreiner/HyperVerITAS.git";
const HYPERVERITAS_REV: &str = "915c48a";
const ZKLP_URL: &str = "https://github.com/tumberger/zk-Location.git";
const ZKLP_REV: &str = "e8e3215";
/// 2^19 pixels, the size the example takes as its argument.
const HYPERVERITAS_LOG_SIZE: &str = "19";
const ZKLP_RESULTS: &str = "benchmarks/raw/bench_ZKLP32_G16_BN254.txt";
const ZKLP_VECTORS: &str = "data/f32/loc2index32.txt";
const WASM_DRIVER: &str = "wasm-fingerprint.mjs";
const STEP_TIMEOUT: Duration = Duration::from_secs(30 * 60);

fn upstream(work: &Path) -> PathBuf {
    // Shared between runs: the sources are pinned, and builds take minutes.
    work.parent()
        .expect("a run's scratch space has a parent")
        .join("upstream")
}

fn hyperveritas(work: &Path) -> PathBuf {
    upstream(work).join("HyperVerITAS/hyperveritas_impl")
}

fn zklp(work: &Path) -> PathBuf {
    upstream(work).join("zk-Location")
}

/// Builds what the operation needs, or says why it cannot be measured.
pub fn prepare(lab: &Lab, op: Op, case: &Case, repository: &Path) -> Result<()> {
    match op {
        Op::UpstreamCrop => prepare_hyperveritas(lab, case),
        Op::UpstreamLocation => prepare_zklp(lab, case),
        Op::WasmFingerprint => prepare_wasm(lab, case, repository),
        _ => Ok(()),
    }
}

fn run(command: &mut Command) -> Result<String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let usage = measure::run(command, STEP_TIMEOUT)?;
    match usage.outcome {
        Exit::Success => Ok(usage.stdout),
        Exit::TimedOut => bail!("{program} timed out"),
        Exit::Failed(why) => bail!("{program}: {why}: {}", crate::run::tail(&usage.stderr)),
    }
}

/// Checks `rev` of `url` out at `dir`, with submodules, unless it is there.
fn clone(url: &str, rev: &str, dir: &Path) -> Result<()> {
    if dir.join(".git").exists() {
        return Ok(());
    }
    let staging = dir.with_extension("cloning");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(dir.parent().context("a clone has a parent")?)?;
    run(Command::new("git")
        .args(["clone", "--quiet", url])
        .arg(&staging))?;
    run(Command::new("git")
        .args(["checkout", "--quiet", rev])
        .current_dir(&staging))?;
    run(Command::new("git")
        .args(["submodule", "update", "--init", "--recursive", "--quiet"])
        .current_dir(&staging))?;
    fs::rename(&staging, dir)?;
    Ok(())
}

fn prepare_hyperveritas(lab: &Lab, case: &Case) -> Result<()> {
    let root = upstream(&lab.work).join("HyperVerITAS");
    clone(HYPERVERITAS_URL, HYPERVERITAS_REV, &root)?;
    let dir = hyperveritas(&lab.work);
    detach_from_workspace(&dir.join("Cargo.toml"))?;
    run(nightly_cargo()?
        .args(["build", "--release", "--example", "hv_crop_pst"])
        .current_dir(&dir))?;
    let original = image::read_png(&lab.fixture(case).join("original.png"))?;
    let crop = crop_proof::crop(&original, case.crop)?;
    write_upstream_image(&original, &dir.join("images/Timings19.json"))?;
    write_upstream_image(&crop, &dir.join("images/Crop19.json"))
}

/// Upstream's dependencies use nightly features. `rustup run` is not enough:
/// scripts/lib.sh puts the pinned toolchain first on PATH, and cargo would find
/// that rustc.
fn nightly_cargo() -> Result<Command> {
    let cargo = run(Command::new("rustup").args(["which", "--toolchain", "nightly", "cargo"]))?;
    let cargo = PathBuf::from(cargo.trim());
    let bin = cargo.parent().context("cargo is in a directory")?;
    let inherited = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(iter::once(bin.to_path_buf()).chain(env::split_paths(&inherited)))?;
    let mut command = Command::new(&cargo);
    command.env("PATH", path);
    Ok(command)
}

/// Under `target`, cargo would take the clone for a member of this workspace.
fn detach_from_workspace(manifest: &Path) -> Result<()> {
    let text = fs::read_to_string(manifest)?;
    if !text.contains("[workspace]") {
        fs::write(manifest, format!("{text}\n[workspace]\n"))?;
    }
    Ok(())
}

/// The JSON layout of upstream's images/helper.py, which calls width "rows".
fn write_upstream_image(image: &RgbImage, path: &Path) -> Result<()> {
    let Size { width, height } = image.size();
    let [r, g, b] = image.channels();
    let body = json!({"rows": width, "cols": height, "R": r, "G": g, "B": b});
    fs::write(path, serde_json::to_vec(&body)?)
        .with_context(|| format!("writing {}", path.display()))
}

pub fn crop(work: &Path, measured: &mut Measured) -> Result<()> {
    let dir = hyperveritas(work);
    let output = run(
        Command::new(dir.join("target/release/examples/hv_crop_pst"))
            .arg(HYPERVERITAS_LOG_SIZE)
            .current_dir(&dir),
    )?;
    parse_hyperveritas(&output, measured)
}

fn parse_hyperveritas(output: &str, measured: &mut Measured) -> Result<()> {
    ensure!(
        output.contains("Verifier passed!: true"),
        "the upstream verifier did not pass"
    );
    let value = |label: &str| -> Result<f64> {
        output
            .lines()
            .find_map(|line| line.strip_prefix(label))
            .and_then(|rest| rest.split_whitespace().next())
            .with_context(|| format!("no {label:?} in the output"))?
            .parse()
            .with_context(|| format!("parsing {label:?}"))
    };
    measured.record("prove", value("PROVER TIME: ")?);
    measured.record("verify", value("VERIFIER TIME: ")?);
    measured.metric("proof_bytes", value("PROOF SIZE: ")? as usize);
    Ok(())
}

fn prepare_zklp(lab: &Lab, case: &Case) -> Result<()> {
    let root = zklp(&lab.work);
    clone(ZKLP_URL, ZKLP_REV, &root)?;
    run(Command::new("go")
        .args(["test", "-c", "-o", "zklp.test", "./loc2index32"])
        .current_dir(&root))?;
    // The benchmark proves every line of the file, so leave one of the
    // reference resolution. Its third column is the resolution in hex.
    let vectors = root.join(ZKLP_VECTORS);
    let text = fs::read_to_string(&vectors)?;
    let line = text
        .lines()
        .find(|line| {
            line.split_whitespace()
                .nth(2)
                .and_then(|field| u8::from_str_radix(field, 16).ok())
                == Some(case.resolution)
        })
        .with_context(|| format!("no vector at resolution {}", case.resolution))?;
    fs::write(&vectors, format!("{line}\n"))?;
    Ok(())
}

pub fn location(work: &Path, measured: &mut Measured) -> Result<()> {
    let root = zklp(work);
    run(Command::new(root.join("zklp.test"))
        .args(["-test.run", "^$", "-test.benchtime", "1x"])
        .args(["-test.bench", "BenchmarkLoc2IndexProof$"])
        .current_dir(root.join("loc2index32")))?;
    let results = fs::read_to_string(root.join(ZKLP_RESULTS))?;
    parse_zklp(&results, measured)
}

/// The last line of the benchmark's file: resolution, index, constraints, and
/// compile, setup, prover and verifier times in microseconds.
fn parse_zklp(results: &str, measured: &mut Measured) -> Result<()> {
    let line = results.lines().last().context("no benchmark results")?;
    let fields: Vec<f64> = line
        .split(", ")
        .map(str::parse)
        .collect::<Result<_, _>>()
        .with_context(|| format!("parsing {line:?}"))?;
    let [_, _, constraints, _, _, prove, verify] = fields[..] else {
        bail!("expected seven columns in {line:?}");
    };
    measured.record("prove", prove / 1e6);
    measured.record("verify", verify / 1e6);
    measured.metric("constraints", constraints as usize);
    Ok(())
}

fn wasm_module(work: &Path) -> PathBuf {
    upstream(work).join("fingerprint.wasm")
}

fn planes(lab: &Lab, case: &Case) -> PathBuf {
    lab.fixture(case).join("planes.bin")
}

fn prepare_wasm(lab: &Lab, case: &Case, repository: &Path) -> Result<()> {
    run(Command::new("node").arg("--version"))?;
    run(Command::new("cargo")
        .args(["build", "--profile", "wasm", "-p", "fingerprint"])
        .args(["--target", "wasm32-unknown-unknown"])
        .current_dir(repository))?;
    fs::create_dir_all(upstream(&lab.work))?;
    fs::copy(
        repository.join("target/wasm32-unknown-unknown/wasm/fingerprint.wasm"),
        wasm_module(&lab.work),
    )?;
    let original = image::read_png(&lab.fixture(case).join("original.png"))?;
    fs::write(planes(lab, case), original.channels().concat())?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct WasmReply {
    seconds: f64,
    /// The hash of all three channels, as the page gets it from the module.
    elements: String,
}

pub fn wasm_fingerprint(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let driver = Path::new(env!("CARGO_MANIFEST_DIR")).join(WASM_DRIVER);
    let reply: WasmReply = serde_json::from_str(&run(Command::new("node")
        .arg(driver)
        .arg(wasm_module(&lab.work))
        .arg(planes(lab, case)))?)?;
    let signed = provenance::capture::Capture::read_public(&lab.fixture(case))?
        .0
        .fingerprint;
    let expected: String = [signed.r, signed.g, signed.b]
        .concat()
        .iter()
        .map(|element| element.trim_start_matches("0x"))
        .collect();
    ensure!(
        reply.elements == expected,
        "the module hashed to another fingerprint"
    );
    measured.record("fingerprint, one thread", reply.seconds);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyperveritas_output_gives_both_times_and_the_size() {
        let output = "starting prover\nPROVER TIME: 10.658 seconds\n\nPROOF SIZE: 49056 bytes\n\n\
                      starting verifier\nVerifier passed!: true\nverifier done!\n\n\
                      VERIFIER TIME: 2.604 seconds\n";
        let mut measured = Measured::default();
        parse_hyperveritas(output, &mut measured).unwrap();
        assert_eq!(measured.phases[0].seconds, 10.658);
        assert_eq!(measured.phases[1].seconds, 2.604);
        assert_eq!(measured.metrics["proof_bytes"], 49056);
    }

    #[test]
    fn a_hyperveritas_run_that_rejects_its_proof_is_a_failure() {
        let output = "PROVER TIME: 1 seconds\nVerifier passed!: false\n";
        assert!(parse_hyperveritas(output, &mut Measured::default()).is_err());
    }

    #[test]
    fn zklp_results_use_the_last_line_in_microseconds() {
        let results = "Resolution, Index, NbConstraints, CompilationTime, SetupTime, ProverTime, VerifierTime\n\
                       15, 3, 19467, 155315, 4110599, 256935, 1346\n\
                       7, 1, 19467, 71680, 696693, 39499, 1289\n";
        let mut measured = Measured::default();
        parse_zklp(results, &mut measured).unwrap();
        assert_eq!(measured.phases[0].seconds, 0.039499);
        assert_eq!(measured.phases[1].seconds, 0.001289);
        assert_eq!(measured.metrics["constraints"], 19467);
    }

    #[test]
    fn zklp_results_without_a_data_line_are_an_error() {
        let header = "Resolution, Index, NbConstraints\n";
        assert!(parse_zklp(header, &mut Measured::default()).is_err());
    }
}
