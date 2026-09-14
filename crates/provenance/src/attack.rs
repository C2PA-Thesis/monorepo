use std::{fs, path::Path};

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Fingerprint, RgbImage};
use serde::Serialize;

use crate::{
    capture::{self, DeviceKey, Receipt, Secrets},
    demo::OTHER_PLACE,
    manifest::{self, Assertion},
    verify::{Check, Verdict},
    Workspace,
};

pub struct Attack {
    pub name: &'static str,
    pub summary: &'static str,
    /// The check that must reject the forged file.
    pub expected: Check,
    needs_other_capture: bool,
    tamper: fn(&mut Evidence, &Forger) -> Result<()>,
}

/// What gets republished: the assertion and the pixels it is attached to.
struct Evidence {
    assertion: Assertion,
    pixels: RgbImage,
}

/// What the attacker is granted: the device key itself, and a second genuine
/// capture of another photo taken somewhere else.
struct Forger {
    device: DeviceKey,
    other: Option<Assertion>,
}

impl Forger {
    fn other(&self) -> &Assertion {
        self.other
            .as_ref()
            .expect("attacks that use another capture request one")
    }
}

pub const ATTACKS: &[Attack] = &[
    Attack {
        name: "signature",
        summary: "flip one bit of the device signature",
        expected: Check::Device,
        needs_other_capture: false,
        tamper: |evidence, _| {
            let mut signature = BASE64.decode(&evidence.assertion.receipt.signature)?;
            *signature.last_mut().context("empty signature")? ^= 1;
            evidence.assertion.receipt.signature = BASE64.encode(signature);
            Ok(())
        },
    },
    Attack {
        name: "fingerprint",
        summary: "swap two fingerprint values without re-signing",
        expected: Check::Device,
        needs_other_capture: false,
        tamper: |evidence, _| {
            evidence.assertion.receipt.fingerprint.r.swap(0, 1);
            Ok(())
        },
    },
    Attack {
        name: "resigned-fingerprint",
        summary: "swap two fingerprint values and re-sign with the device key",
        expected: Check::Image,
        needs_other_capture: false,
        tamper: |evidence, forger| {
            evidence.assertion.receipt.fingerprint.r.swap(0, 1);
            evidence.assertion.receipt.resign(&forger.device)
        },
    },
    Attack {
        name: "pixel",
        summary: "change one published pixel",
        expected: Check::Image,
        needs_other_capture: false,
        tamper: |evidence, _| {
            let mut channels = evidence.pixels.channels().clone();
            channels[0][0] ^= 1;
            evidence.pixels = RgbImage::new(evidence.pixels.size(), channels)?;
            Ok(())
        },
    },
    Attack {
        name: "region",
        summary: "claim another cell for the same location proof",
        expected: Check::Location,
        needs_other_capture: true,
        tamper: |evidence, forger| {
            evidence.assertion.cell = forger.other().cell.clone();
            Ok(())
        },
    },
    Attack {
        name: "cross-photo-location",
        summary: "attach the location proof of another photo taken elsewhere",
        expected: Check::Location,
        needs_other_capture: true,
        tamper: |evidence, forger| {
            let other = forger.other();
            evidence.assertion.cell = other.cell.clone();
            evidence.assertion.location_proof = other.location_proof.clone();
            Ok(())
        },
    },
    Attack {
        name: "cross-photo-receipt",
        summary: "attach the receipt and location proof of another photo, keeping this crop proof",
        expected: Check::Image,
        needs_other_capture: true,
        tamper: |evidence, forger| {
            let other = forger.other();
            evidence.assertion.receipt = other.receipt.clone();
            evidence.assertion.cell = other.cell.clone();
            evidence.assertion.location_proof = other.location_proof.clone();
            Ok(())
        },
    },
];

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub attack: &'static str,
    pub expected: Check,
    pub verdict: Verdict,
}

impl Outcome {
    pub fn as_expected(&self) -> bool {
        self.verdict
            .rejected
            .as_ref()
            .is_some_and(|step| step.check == self.expected)
    }
}

/// Replays attacks against a demo run. Each writes a freshly C2PA-signed PNG
/// to `<run>/attacks/<name>/signed.png` and verifies it.
pub fn run(
    workspace: &Workspace,
    run: &Path,
    only: Option<&str>,
    on: &mut dyn FnMut(&Outcome),
) -> Result<Vec<Outcome>> {
    let selected: Vec<&Attack> = ATTACKS
        .iter()
        .filter(|attack| only.is_none_or(|name| attack.name == name))
        .collect();
    ensure!(
        !selected.is_empty(),
        "unknown attack; choose one of {}",
        ATTACKS
            .iter()
            .map(|attack| attack.name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let signed = run.join("signed.png");
    let assertion = manifest::read(&signed)
        .with_context(|| format!("reading {}; run `provenance demo` first", signed.display()))?;
    let pixels = capture::read_png(&signed)?;
    let device = workspace.device_key()?;
    let other = match selected.iter().any(|attack| attack.needs_other_capture) {
        true => Some(other_capture(workspace, run, &device)?),
        false => None,
    };
    let forger = Forger { device, other };
    let editor = workspace.editor()?;
    let verifier = workspace.verifier()?;

    let mut outcomes = Vec::with_capacity(selected.len());
    for attack in selected {
        let dir = run.join("attacks").join(attack.name);
        fs::create_dir_all(&dir)?;
        let mut evidence = Evidence {
            assertion: assertion.clone(),
            pixels: pixels.clone(),
        };
        (attack.tamper)(&mut evidence, &forger)?;
        let unsigned = dir.join("crop.png");
        capture::write_png(&evidence.pixels, &unsigned)?;
        let forged = dir.join("signed.png");
        editor.sign(&unsigned, &evidence.assertion, &forged)?;
        let outcome = Outcome {
            attack: attack.name,
            expected: attack.expected,
            verdict: verifier.verify(&forged, None),
        };
        on(&outcome);
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// A genuine capture of a different photo (the original mirrored) at
/// another place, with its own receipt and location proof.
fn other_capture(workspace: &Workspace, run: &Path, device: &DeviceKey) -> Result<Assertion> {
    let original = capture::read_png(&run.join("original.png"))?;
    let width = original.size().width;
    let mirrored = original.channels().each_ref().map(|channel| {
        channel
            .chunks(width)
            .flat_map(|row| row.iter().rev())
            .copied()
            .collect()
    });
    let mirrored = RgbImage::new(original.size(), mirrored)?;

    let tool = workspace.location_tool();
    let secrets = Secrets::new(OTHER_PLACE.latitude, OTHER_PLACE.longitude);
    let receipt = Receipt::sign(Fingerprint::of(&mirrored)?, tool.commit(&secrets)?, device)?;
    let proof = tool.prove(&secrets, OTHER_PLACE.cell)?;
    Ok(Assertion::new(
        receipt,
        OTHER_PLACE.cell.to_string(),
        proof.proof,
        String::new(),
    ))
}
