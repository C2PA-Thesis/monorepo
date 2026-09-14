use std::{fs, path::Path, time::Instant};

use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, ProverParams, CROP};
use serde::Serialize;

use crate::{
    capture::{self, short, Receipt, Secrets},
    manifest::Assertion,
    Workspace,
};

pub struct Place {
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
    /// Resolution-7 H3 cell containing the coordinate.
    pub cell: &'static str,
}

/// Public test coordinate, not a real capture location.
pub const DEMO_PLACE: Place = Place {
    name: "UTDT, Buenos Aires",
    latitude: -34.5478,
    longitude: -58.4462,
    cell: "87c2e3020ffffff",
};

/// Where the attacks' second photo claims to have been taken.
pub const OTHER_PLACE: Place = Place {
    name: "Tokyo Station",
    latitude: 35.6812,
    longitude: 139.7671,
    cell: "872f5a32dffffff",
};

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

/// Captures `photo` at the given coordinate, publishes its left half with
/// both proofs as `out/signed.png`, and verifies it. `original.png` and
/// `secrets.json` in `out` are private to the photographer.
pub fn run(
    workspace: &Workspace,
    photo: &Path,
    (latitude, longitude): (f64, f64),
    cell: &str,
    out: &Path,
    on: &mut dyn FnMut(Event),
) -> Result<()> {
    let tool = workspace.location_tool();
    tool.region(cell)?;
    let device = workspace.device_key()?;
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;

    let (original, secrets, receipt) = stage(on, Stage::Capture, || {
        let original = capture::load_photo(photo)?;
        capture::write_png(&original, &out.join("original.png"))?;
        let secrets = Secrets::new(latitude, longitude);
        capture::write_private(
            &out.join("secrets.json"),
            &serde_json::to_vec_pretty(&secrets)?,
        )?;
        let receipt = Receipt::sign(Fingerprint::of(&original)?, tool.commit(&secrets)?, &device)?;
        fs::write(
            out.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
        let detail = format!(
            "fingerprint and envelope signed by device {}",
            short(&receipt.device)
        );
        Ok(((original, secrets, receipt), detail))
    })?;

    let crop_png = out.join("crop.png");
    let crop = stage(on, Stage::Crop, || {
        let crop = original.left_half()?;
        capture::write_png(&crop, &crop_png)?;
        Ok((crop, format!("kept the {CROP} left half")))
    })?;

    let location_proof = stage(on, Stage::LocationProof, || {
        let proof = tool.prove(&secrets, cell)?;
        ensure!(
            proof.envelope == receipt.envelope,
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
        let proof = crop_proof::prove(&params, &original, &crop, &receipt.fingerprint)?;
        Ok((
            BASE64.encode(&proof),
            format!("HyperVerITAS PST proof, {} KB", proof.len() / 1000),
        ))
    })?;

    let signed = out.join("signed.png");
    stage(on, Stage::Publish, || {
        let assertion = Assertion::new(
            receipt.clone(),
            cell.to_string(),
            location_proof,
            image_proof,
        );
        workspace.editor()?.sign(&crop_png, &assertion, &signed)?;
        Ok((
            (),
            format!("{} KB, C2PA-signed", fs::metadata(&signed)?.len() / 1000),
        ))
    })?;

    stage(on, Stage::Verify, || {
        let verdict = workspace.verifier()?.verify(&signed, Some(cell));
        match verdict.rejected {
            Some(step) => bail!(
                "{} rejected the published file: {}",
                step.check.title(),
                step.detail
            ),
            None => Ok(((), "all reader checks passed".to_string())),
        }
    })
}

fn stage<T>(
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
