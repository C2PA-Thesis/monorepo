//! The operations a sample runs, each in a process of its own. Every one
//! checks its result, so a broken pipeline fails the sample instead of timing
//! the wrong work.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, ProverParams, RgbImage, Size, VerifierParams};
use provenance::{
    attack,
    capture::Capture,
    demo::{self, DEMO_PLACE},
    image, manifest, publish, verify, Workspace,
};
use serde::{Deserialize, Serialize};

use crate::{
    baseline,
    suite::{Case, Op},
};

const SIGNED: &str = "signed.png";
const ORIGINAL: &str = "original.png";
const CROP: &str = "crop.png";

/// What a sample asks its process to do.
#[derive(Debug, Serialize, Deserialize)]
pub struct Task {
    pub op: Op,
    pub case: Case,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    pub seconds: f64,
}

/// What an operation reports from inside its process.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Measured {
    pub phases: Vec<Phase>,
    pub metrics: BTreeMap<String, u64>,
}

impl Measured {
    pub fn record(&mut self, name: &str, seconds: f64) {
        self.phases.push(Phase {
            name: name.to_string(),
            seconds,
        });
    }

    pub fn time<T>(&mut self, name: &str, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let started = Instant::now();
        let value = work()?;
        self.record(name, started.elapsed().as_secs_f64());
        Ok(value)
    }

    pub fn metric(&mut self, name: &str, value: usize) {
        self.metrics.insert(name.to_string(), value as u64);
    }
}

/// The proof parameters and keys, and the scratch space of a run.
pub struct Lab {
    pub workspace: Workspace,
    pub work: PathBuf,
}

impl Lab {
    pub fn fixture(&self, case: &Case) -> PathBuf {
        self.work.join("fixtures").join(case.key())
    }

    fn read(&self, case: &Case) -> Result<Capture> {
        Capture::read(&self.fixture(case))
    }

    fn assertion(&self, case: &Case) -> Result<manifest::Assertion> {
        manifest::read(&self.fixture(case).join(SIGNED))
    }
}

pub fn execute(lab: &Lab, task: &Task) -> Result<Measured> {
    let case = &task.case;
    let mut measured = Measured::default();
    match task.op {
        Op::Prepare => prepare(lab, case)?,
        Op::Pipeline => pipeline(lab, case, &mut measured)?,
        Op::Fingerprint => fingerprint(lab, case, &mut measured)?,
        Op::Capture => capture(lab, case, &mut measured)?,
        Op::LocationProve => location_prove(lab, case, &mut measured)?,
        Op::LocationVerify => location_verify(lab, case, &mut measured)?,
        Op::CropProve => crop_prove(lab, case, &mut measured)?,
        Op::CropVerify => crop_verify(lab, case, &mut measured)?,
        Op::Sign => sign(lab, case, &mut measured)?,
        Op::Read => read(lab, case, &mut measured)?,
        Op::Attacks => attacks(lab, case, &mut measured)?,
        Op::UpstreamCrop => baseline::crop(&lab.work, &mut measured)?,
        Op::UpstreamLocation => baseline::location(&lab.work, &mut measured)?,
        Op::WasmFingerprint => baseline::wasm_fingerprint(lab, case, &mut measured)?,
        Op::PhoneFingerprint | Op::PhoneEnvelope | Op::PhoneLocationProve | Op::PhoneCropProve => {
            anyhow::bail!("phone operations run on the phone")
        }
    }
    Ok(measured)
}

/// Publishes the case once, into a directory that appears only when complete.
fn prepare(lab: &Lab, case: &Case) -> Result<()> {
    let target = lab.fixture(case);
    if target.join(SIGNED).exists() {
        return Ok(());
    }
    let parent = target.parent().context("a fixture has a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let staging = tempfile::tempdir_in(parent)?;

    let workspace = &lab.workspace;
    let tool = workspace.location_tool();
    let photo = image::load_photo(&workspace.sample_photo())?;
    let mut capture = Capture::simulate(
        &tool,
        &workspace.device_key()?,
        resample(&photo, case.image)?,
        DEMO_PLACE.coordinate,
        &tool.cell(DEMO_PLACE.coordinate, case.resolution)?.cell,
        case.crop,
    )?;
    capture.summary.turn = case.turn;
    capture.write(staging.path())?;
    publish::run(workspace, &capture, staging.path(), &mut |_| {})?;
    fs::rename(staging.keep(), &target).with_context(|| format!("creating {}", target.display()))
}

/// Nearest-neighbour, because a sweep needs real photo content at exact sizes
/// and the proofs do not depend on how pretty it is.
fn resample(photo: &RgbImage, size: Size) -> Result<RgbImage> {
    let from = photo.size();
    let source = |index: usize| {
        let (x, y) = (index % size.width, index / size.width);
        (y * from.height / size.height) * from.width + x * from.width / size.width
    };
    let channels = photo
        .channels()
        .each_ref()
        .map(|channel| (0..size.pixels()).map(|i| channel[source(i)]).collect());
    RgbImage::new(size, channels)
}

fn pipeline(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let fixture = lab.fixture(case);
    let (_, summary) = Capture::read_public(&fixture)?;
    let out = tempfile::tempdir_in(&lab.work)?;
    demo::run(
        &lab.workspace,
        &fixture.join(ORIGINAL),
        DEMO_PLACE.coordinate,
        &summary.cell,
        case.turn,
        out.path(),
        &mut |event| {
            if let publish::Event::Finished { stage, seconds, .. } = event {
                measured.record(stage.title(), seconds);
            }
        },
    )?;
    measured.metric("signed_bytes", size_of(&out.path().join(SIGNED))?);
    Ok(())
}

fn fingerprint(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let capture = lab.read(case)?;
    let fingerprint = measured.time("fingerprint", || Fingerprint::of(&capture.original))?;
    ensure!(
        fingerprint == capture.receipt.fingerprint,
        "the fingerprint differs from the one the device signed"
    );
    Ok(())
}

fn capture(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let existing = lab.read(case)?;
    let workspace = &lab.workspace;
    let tool = workspace.location_tool();
    let device = workspace.device_key()?;
    let fresh = measured.time("capture", || {
        Capture::simulate(
            &tool,
            &device,
            existing.original.clone(),
            DEMO_PLACE.coordinate,
            &existing.summary.cell,
            case.crop,
        )
    })?;
    ensure!(
        fresh.receipt.fingerprint == existing.receipt.fingerprint,
        "the capture fingerprinted other pixels"
    );
    Ok(())
}

fn location_prove(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let capture = lab.read(case)?;
    let secrets = capture.secrets().context("the fixture holds its secrets")?;
    let tool = lab.workspace.location_tool();
    let proof = measured.time("prove", || tool.prove(secrets, &capture.summary.cell))?;
    ensure!(
        proof.envelope == capture.receipt.envelope,
        "the proof is for another envelope"
    );
    measured.metric("proof_bytes", BASE64.decode(&proof.proof)?.len());
    Ok(())
}

fn location_verify(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let assertion = lab.assertion(case)?;
    let tool = lab.workspace.location_tool();
    measured.time("verify", || {
        tool.verify(
            &assertion.cell,
            &assertion.receipt.envelope,
            &assertion.location_proof,
        )
    })
}

fn crop_prove(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let capture = lab.read(case)?;
    let published = crop_proof::edit(&capture.original, case.crop, case.turn)?;
    let params = measured.time("load parameters", || {
        ProverParams::load(&lab.workspace.crop_params())
    })?;
    let proof = measured.time("prove", || {
        crop_proof::prove(
            &params,
            &capture.original,
            &published,
            case.crop,
            case.turn,
            &capture.receipt.fingerprint,
        )
    })?;
    measured.metric("proof_bytes", proof.len());
    Ok(())
}

fn crop_verify(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let assertion = lab.assertion(case)?;
    let published = image::read_png(&lab.fixture(case).join(SIGNED))?;
    let proof = BASE64.decode(&assertion.image_proof)?;
    let params = measured.time("load parameters", || {
        VerifierParams::load(&lab.workspace.crop_params())
    })?;
    measured.time("verify", || {
        crop_proof::verify(
            &params,
            &published,
            assertion.crop,
            assertion.turn,
            &assertion.receipt.fingerprint,
            &proof,
        )
    })
}

fn sign(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let assertion = lab.assertion(case)?;
    let editor = lab.workspace.editor()?;
    let crop = lab.fixture(case).join(CROP);
    let out = tempfile::tempdir_in(&lab.work)?;
    let signed = out.path().join(SIGNED);
    measured.time("sign", || editor.sign(&crop, &assertion, &signed))?;
    let (signed, crop) = (size_of(&signed)?, size_of(&crop)?);
    measured.metric("signed_bytes", signed);
    measured.metric("manifest_bytes", signed.saturating_sub(crop));
    Ok(())
}

fn read(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let signed = lab.fixture(case).join(SIGNED);
    let verifier = measured.time("load keys and parameters", || lab.workspace.verifier())?;
    let verdict = verifier.verify(&signed, None, &mut |event| {
        if let verify::Event::Passed { check, seconds, .. } = event {
            measured.record(check.title(), seconds);
        }
    });
    if let Some(step) = verdict.rejected {
        anyhow::bail!("{} rejected the file: {}", step.check.title(), step.detail);
    }
    measured.metric("signed_bytes", size_of(&signed)?);
    Ok(())
}

fn attacks(lab: &Lab, case: &Case, measured: &mut Measured) -> Result<()> {
    let run = lab.fixture(case);
    let outcomes = measured.time("attacks", || {
        attack::run(&lab.workspace, &run, None, &mut |_| {})
    })?;
    let surprises: Vec<&str> = outcomes
        .iter()
        .filter(|outcome| !outcome.as_expected())
        .map(|outcome| outcome.attack)
        .collect();
    ensure!(surprises.is_empty(), "unexpected outcome: {surprises:?}");
    measured.metric("attacks", outcomes.len());
    Ok(())
}

fn size_of(path: &Path) -> Result<usize> {
    let metadata = fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(metadata.len() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(size: Size) -> RgbImage {
        let channel: Vec<u8> = (0..size.pixels()).map(|i| (i % 251) as u8).collect();
        RgbImage::new(size, [channel.clone(), channel.clone(), channel]).unwrap()
    }

    #[test]
    fn resampling_reaches_the_exact_size_both_ways() {
        let photo = gradient(Size {
            width: 40,
            height: 30,
        });
        for size in [
            Size {
                width: 7,
                height: 5,
            },
            Size {
                width: 64,
                height: 64,
            },
        ] {
            assert_eq!(resample(&photo, size).unwrap().size(), size);
        }
    }

    #[test]
    fn resampling_to_the_same_size_changes_nothing() {
        let photo = gradient(Size {
            width: 16,
            height: 9,
        });
        assert_eq!(resample(&photo, photo.size()).unwrap(), photo);
    }

    #[test]
    fn a_timed_failure_records_no_phase() {
        let mut measured = Measured::default();
        let failure: Result<()> = measured.time("work", || anyhow::bail!("no"));
        assert!(failure.is_err());
        assert!(measured.phases.is_empty());
    }
}
