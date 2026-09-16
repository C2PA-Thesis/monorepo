//! The end-to-end demo: a simulated capture, published and verified.

use std::path::Path;

use anyhow::{bail, Result};
use crop_proof::Rect;

use crate::{
    capture::{Capture, Coordinate},
    image,
    publish::{self, stage, Event, Stage},
    receipt::short,
    Workspace,
};

pub struct Place {
    pub name: &'static str,
    pub coordinate: Coordinate,
    /// Resolution-7 H3 cell containing the coordinate.
    pub cell: &'static str,
}

/// Public test coordinate, not a real capture location.
pub const DEMO_PLACE: Place = Place {
    name: "UTDT, Buenos Aires",
    coordinate: Coordinate {
        latitude: -34.5478,
        longitude: -58.4462,
    },
    cell: "87c2e3020ffffff",
};

/// Where the attacks' second photo claims to have been taken.
pub const OTHER_PLACE: Place = Place {
    name: "Tokyo Station",
    coordinate: Coordinate {
        latitude: 35.6812,
        longitude: 139.7671,
    },
    cell: "872f5a32dffffff",
};

pub const STAGES: [Stage; 6] = [
    Stage::Capture,
    Stage::Crop,
    Stage::LocationProof,
    Stage::CropProof,
    Stage::Publish,
    Stage::Verify,
];

/// Captures `photo` at `coordinate` with the laptop's device key, publishes
/// its left half with both proofs as `out/signed.png`, and verifies it. The
/// capture files in `out` are private to the photographer.
pub fn run(
    workspace: &Workspace,
    photo: &Path,
    coordinate: Coordinate,
    cell: &str,
    out: &Path,
    on: &mut dyn FnMut(Event),
) -> Result<()> {
    let capture = stage(on, Stage::Capture, || {
        let capture = Capture::simulate(
            &workspace.location_tool(),
            &workspace.device_key()?,
            image::load_photo(photo)?,
            coordinate,
            cell,
            Rect::LEFT_HALF,
        )?;
        capture.write(out)?;
        let detail = format!(
            "fingerprint and envelope signed by device {}",
            short(&capture.receipt.device)
        );
        Ok((capture, detail))
    })?;

    let signed = publish::run(workspace, &capture, out, on)?;

    stage(on, Stage::Verify, || {
        let verdict = workspace
            .verifier()?
            .verify(&signed, Some(cell), &mut |_| {});
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
