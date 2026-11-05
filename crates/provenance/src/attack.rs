use std::{fs, path::Path};

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Rect, RgbImage, Turn};
use serde::Serialize;

use crate::{
    capture::{Capture, Secrets},
    demo::OTHER_PLACE,
    image,
    location::LocationTool,
    manifest::{self, Assertion},
    receipt::{DeviceKey, Receipt},
    verify::{Check, Verdict},
    Workspace,
};

/// Resolution-13 cell of the demo coordinate, about 44 m^2. It is the finest
/// resolution where the circuit's float32 mapping still matches H3 there.
const FINE_CELL: &str = "8dc2e30205ad07f";

pub struct Attack {
    pub name: &'static str,
    pub summary: &'static str,
    pub expected: Expected,
    needs_other_capture: bool,
    tamper: fn(&mut Evidence, &Forger) -> Result<()>,
}

/// Most attacks must be rejected by one check. The accepted ones show what the
/// pipeline trusts instead of checking: the device key and what it signs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "check", rename_all = "kebab-case")]
pub enum Expected {
    Rejected(Check),
    Accepted,
}

/// What gets republished: the assertion and the pixels it is attached to.
struct Evidence {
    assertion: Assertion,
    pixels: RgbImage,
}

/// What the attacker is granted: the device key itself, the photographer's
/// secrets, the location prover, and a second genuine capture of another photo
/// taken somewhere else.
struct Forger {
    device: DeviceKey,
    secrets: Secrets,
    location: LocationTool,
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
        expected: Expected::Rejected(Check::Device),
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
        expected: Expected::Rejected(Check::Device),
        needs_other_capture: false,
        tamper: |evidence, _| {
            evidence.assertion.receipt.fingerprint.r.swap(0, 1);
            Ok(())
        },
    },
    Attack {
        name: "resigned-fingerprint",
        summary: "swap two fingerprint values and re-sign with the device key",
        expected: Expected::Rejected(Check::Image),
        needs_other_capture: false,
        tamper: |evidence, forger| {
            evidence.assertion.receipt.fingerprint.r.swap(0, 1);
            evidence.assertion.receipt.resign(&forger.device)
        },
    },
    Attack {
        name: "pixel",
        summary: "change one published pixel",
        expected: Expected::Rejected(Check::Image),
        needs_other_capture: false,
        tamper: |evidence, _| {
            let mut channels = evidence.pixels.channels().clone();
            channels[0][0] ^= 1;
            evidence.pixels = RgbImage::new(evidence.pixels.size(), channels)?;
            Ok(())
        },
    },
    Attack {
        name: "rectangle",
        summary: "claim the same pixels are one column to the right",
        expected: Expected::Rejected(Check::Image),
        needs_other_capture: false,
        tamper: |evidence, _| {
            evidence.assertion.crop.x += 1;
            Ok(())
        },
    },
    Attack {
        name: "turn",
        summary: "turn the published pixels half a turn and claim that turn",
        expected: Expected::Rejected(Check::Image),
        needs_other_capture: false,
        tamper: |evidence, _| {
            let half = Turn::new(2)?;
            evidence.pixels = half.apply(&evidence.pixels)?;
            evidence.assertion.turn = half;
            Ok(())
        },
    },
    Attack {
        name: "region",
        summary: "claim another cell for the same location proof",
        expected: Expected::Rejected(Check::Location),
        needs_other_capture: true,
        tamper: |evidence, forger| {
            evidence.assertion.cell = forger.other().cell.clone();
            Ok(())
        },
    },
    Attack {
        name: "cross-photo-location",
        summary: "attach the location proof of another photo taken elsewhere",
        expected: Expected::Rejected(Check::Location),
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
        expected: Expected::Rejected(Check::Image),
        needs_other_capture: true,
        tamper: |evidence, forger| {
            let other = forger.other();
            evidence.assertion.receipt = other.receipt.clone();
            evidence.assertion.cell = other.cell.clone();
            evidence.assertion.location_proof = other.location_proof.clone();
            Ok(())
        },
    },
    Attack {
        name: "signed-elsewhere",
        summary: "with the device key, sign this photo at Tokyo and prove that cell",
        expected: Expected::Accepted,
        needs_other_capture: false,
        tamper: |evidence, forger| {
            let secrets = Secrets::new(OTHER_PLACE.coordinate);
            let receipt = &evidence.assertion.receipt;
            let envelope = forger.location.commit(&secrets)?;
            evidence.assertion.receipt = Receipt::sign(
                receipt.fingerprint.clone(),
                envelope,
                receipt.captured_at.clone(),
                &forger.device,
            )?;
            evidence.assertion.cell = OTHER_PLACE.cell.to_string();
            evidence.assertion.location_proof =
                forger.location.prove(&secrets, OTHER_PLACE.cell)?.proof;
            Ok(())
        },
    },
    Attack {
        name: "fine-cell",
        summary: "publish the honest coordinate in a resolution-13 cell, about 44 m^2",
        expected: Expected::Rejected(Check::Location),
        needs_other_capture: false,
        tamper: |evidence, forger| {
            evidence.assertion.cell = FINE_CELL.to_string();
            evidence.assertion.location_proof =
                forger.location.prove(&forger.secrets, FINE_CELL)?.proof;
            Ok(())
        },
    },
];

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub attack: &'static str,
    pub expected: Expected,
    pub verdict: Verdict,
}

impl Outcome {
    pub fn as_expected(&self) -> bool {
        match (self.expected, &self.verdict.rejected) {
            (Expected::Rejected(check), Some(step)) => step.check == check,
            (Expected::Accepted, None) => true,
            _ => false,
        }
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
    let pixels = image::read_png(&signed)?;
    let device = workspace.device_key()?;
    let other = match selected.iter().any(|attack| attack.needs_other_capture) {
        true => Some(other_capture(workspace, run, &device)?),
        false => None,
    };
    let secrets_path = run.join("secrets.json");
    let secrets = serde_json::from_slice(
        &fs::read(&secrets_path).with_context(|| format!("reading {}", secrets_path.display()))?,
    )?;
    let forger = Forger {
        device,
        secrets,
        location: workspace.location_tool(),
        other,
    };
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
        image::write_png(&evidence.pixels, &unsigned)?;
        let forged = dir.join("signed.png");
        editor.sign(&unsigned, &evidence.assertion, &forged)?;
        let outcome = Outcome {
            attack: attack.name,
            expected: attack.expected,
            verdict: verifier.verify(&forged, None, &mut |_| {}),
        };
        on(&outcome);
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// A genuine capture of a different photo (the original mirrored) at
/// another place, with its own receipt and location proof.
fn other_capture(workspace: &Workspace, run: &Path, device: &DeviceKey) -> Result<Assertion> {
    let original = image::read_png(&run.join("original.png"))?;
    let width = original.size().width;
    let mirrored = original.channels().each_ref().map(|channel| {
        channel
            .chunks(width)
            .flat_map(|row| row.iter().rev())
            .copied()
            .collect()
    });
    let mirrored = RgbImage::new(original.size(), mirrored)?;
    let crop = Rect::left_half(mirrored.size());

    let tool = workspace.location_tool();
    let capture = Capture::simulate(
        &tool,
        device,
        mirrored,
        OTHER_PLACE.coordinate,
        OTHER_PLACE.cell,
        crop,
    )?;
    let secrets = capture
        .secrets()
        .context("a simulated capture holds its secrets")?;
    let proof = tool.prove(secrets, OTHER_PLACE.cell)?;
    Ok(Assertion::new(
        capture.receipt,
        OTHER_PLACE.cell.to_string(),
        crop,
        Turn::NONE,
        proof.proof,
        String::new(),
    ))
}
