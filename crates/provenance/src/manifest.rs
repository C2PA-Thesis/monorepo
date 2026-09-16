use std::{fs, path::Path};

use anyhow::{ensure, Context, Result};
use c2pa::{create_signer, Builder, Reader, SigningAlg, ValidationState};
use crop_proof::Rect;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::receipt::Receipt;

/// Label of the custom assertion carrying the receipt and both proofs.
pub const LABEL: &str = "edu.utdt.td8.zkloc";
const VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub version: u32,
    pub receipt: Receipt,
    /// H3 cell the location proof claims.
    pub cell: String,
    /// The rectangle of the original the crop proof claims these pixels are.
    pub crop: Rect,
    /// Base64 BN254 Groth16 proof from location-proof.
    pub location_proof: String,
    /// Base64 HyperVerITAS PST proof from crop-proof.
    pub image_proof: String,
}

impl Assertion {
    pub fn new(
        receipt: Receipt,
        cell: String,
        crop: Rect,
        location_proof: String,
        image_proof: String,
    ) -> Self {
        Self {
            version: VERSION,
            receipt,
            cell,
            crop,
            location_proof,
            image_proof,
        }
    }
}

/// The identity that signs the C2PA manifest when the crop is published.
/// It is separate from the device key that signed the capture.
pub struct Editor {
    certificates: Vec<u8>,
    private_key: Vec<u8>,
}

impl Editor {
    pub fn load(certificates: &Path, private_key: &Path) -> Result<Self> {
        let read =
            |path: &Path| fs::read(path).with_context(|| format!("reading {}", path.display()));
        Ok(Self {
            certificates: read(certificates)?,
            private_key: read(private_key)?,
        })
    }

    /// Writes `output`: the PNG `image` with a manifest carrying `assertion`.
    pub fn sign(&self, image: &Path, assertion: &Assertion, output: &Path) -> Result<()> {
        let definition = json!({
            "claim_generator_info": [{"name": "provenance", "version": env!("CARGO_PKG_VERSION")}],
            "assertions": [{
                "label": "c2pa.actions",
                "data": {"actions": [
                    {"action": "c2pa.created", "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture"},
                    {"action": "c2pa.cropped"},
                ]},
            }],
        });
        let mut builder =
            Builder::from_context(c2pa::Context::new()).with_definition(definition)?;
        builder.add_assertion(LABEL, assertion)?;
        let signer = create_signer::from_keys(
            &self.certificates,
            &self.private_key,
            SigningAlg::Es256,
            None,
        )?;
        // sign_file refuses to overwrite its output.
        if output.exists() {
            fs::remove_file(output)?;
        }
        builder.sign_file(signer.as_ref(), image, output)?;
        Ok(())
    }
}

/// The assertion in a signed file, once C2PA validation has passed.
pub fn read(signed: &Path) -> Result<Assertion> {
    let reader = Reader::from_context(c2pa::Context::new())
        .with_file(signed)
        .with_context(|| format!("reading the C2PA manifest of {}", signed.display()))?;
    let state = reader.validation_state();
    ensure!(
        matches!(state, ValidationState::Valid | ValidationState::Trusted),
        "C2PA validation state is {state:?}"
    );
    let manifest = reader
        .active_manifest()
        .context("the file has no active C2PA manifest")?;
    let count = manifest
        .assertions()
        .iter()
        .filter(|assertion| assertion.label() == LABEL)
        .count();
    ensure!(count == 1, "expected one {LABEL} assertion, found {count}");
    Ok(manifest.find_assertion(LABEL)?)
}
