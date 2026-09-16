//! From a capture to a published file: crop, both proofs, and the C2PA manifest.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{left_half, ProverParams, CROP};
use serde::Serialize;

use crate::{capture::Capture, image, manifest::Assertion, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Capture,
    Crop,
    LocationProof,
    CropProof,
    Publish,
    Verify,
}

impl Stage {
    pub fn title(self) -> &'static str {
        match self {
            Stage::Capture => "capture",
            Stage::Crop => "crop",
            Stage::LocationProof => "location proof",
            Stage::CropProof => "crop proof",
            Stage::Publish => "publish",
            Stage::Verify => "verify",
        }
    }
}

/// The stages `run` goes through, in order.
pub const STAGES: [Stage; 4] = [
    Stage::Crop,
    Stage::LocationProof,
    Stage::CropProof,
    Stage::Publish,
];

#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    Started {
        stage: Stage,
    },
    Finished {
        stage: Stage,
        detail: String,
        seconds: f64,
    },
}

/// Publishes the left half of `capture` with both proofs as `out/signed.png`,
/// keeping the unsigned crop next to it, and returns the signed path.
pub fn run(
    workspace: &Workspace,
    capture: &Capture,
    out: &Path,
    on: &mut dyn FnMut(Event),
) -> Result<PathBuf> {
    let tool = workspace.location_tool();
    let cell = &capture.summary.cell;
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;

    let crop_png = out.join("crop.png");
    let crop = stage(on, Stage::Crop, || {
        let crop = left_half(&capture.original)?;
        image::write_png(&crop, &crop_png)?;
        Ok((crop, format!("kept the {CROP} left half")))
    })?;

    let location_proof = stage(on, Stage::LocationProof, || {
        let proof = tool.prove(&capture.secrets, cell)?;
        ensure!(
            proof.envelope == capture.receipt.envelope,
            "location-proof committed to another envelope"
        );
        let size = BASE64.decode(&proof.proof)?.len();
        Ok((
            proof.proof,
            format!("Groth16 proof for cell {cell}, {size} bytes"),
        ))
    })?;

    let image_proof = stage(on, Stage::CropProof, || {
        let params = ProverParams::load(&workspace.crop_params())?;
        let proof = crop_proof::prove(
            &params,
            &capture.original,
            &crop,
            &capture.receipt.fingerprint,
        )?;
        Ok((
            BASE64.encode(&proof),
            format!("HyperVerITAS PST proof, {} KB", proof.len() / 1000),
        ))
    })?;

    let signed = out.join("signed.png");
    stage(on, Stage::Publish, || {
        let assertion = Assertion::new(
            capture.receipt.clone(),
            cell.clone(),
            location_proof,
            image_proof,
        );
        workspace.editor()?.sign(&crop_png, &assertion, &signed)?;
        Ok((
            (),
            format!("{} KB, C2PA-signed", fs::metadata(&signed)?.len() / 1000),
        ))
    })?;
    Ok(signed)
}

/// Runs `work` between a started and a finished event, timing it.
pub fn stage<T>(
    on: &mut dyn FnMut(Event),
    stage: Stage,
    work: impl FnOnce() -> Result<(T, String)>,
) -> Result<T> {
    on(Event::Started { stage });
    let started = Instant::now();
    let (value, detail) = work().with_context(|| format!("{} failed", stage.title()))?;
    on(Event::Finished {
        stage,
        detail,
        seconds: started.elapsed().as_secs_f64(),
    });
    Ok(value)
}
