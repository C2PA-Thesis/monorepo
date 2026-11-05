use std::{fs, path::Path};

use anyhow::{bail, ensure, Context, Result};
use c2pa::{create_signer, Builder, Reader, SigningAlg, ValidationState};
use crop_proof::{Rect, Turn};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::receipt::Receipt;

/// Label of the custom assertion carrying the receipt and both proofs. C2PA
/// namespaces start with the defining entity's domain (§6.2.1); this one is
/// c2pa-thesis.github.io, which the project's GitHub organization controls.
pub const LABEL: &str = "io.github.c2pa-thesis.zkloc";
/// The label before, under the university's domain, which is not ours to use.
/// Still read so files published with it keep verifying.
pub const LEGACY_LABEL: &str = "edu.utdt.td8.zkloc";
/// 6 adds the turn. A version 5 file has none and still verifies, since the
/// crop proof hashes no turn into its statement when there is none. Version 5
/// allowed any original up to 2^19 pixels and bound the crop proof to its
/// whole statement; version 4 added the trigonometric hints to the envelope.
pub const VERSION: u32 = 6;
const NO_TURN_VERSION: u32 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub version: u32,
    pub receipt: Receipt,
    /// H3 cell the location proof claims.
    pub cell: String,
    /// The rectangle of the original the crop proof claims these pixels are.
    pub crop: Rect,
    /// Quarter turns clockwise applied after the crop. Absent before version 6.
    #[serde(default)]
    pub turn: Turn,
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
        turn: Turn,
        location_proof: String,
        image_proof: String,
    ) -> Self {
        Self {
            version: VERSION,
            receipt,
            cell,
            crop,
            turn,
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
        self.sign_as(LABEL, image, assertion, output)
    }

    /// `sign` under another label, for tests of what the reader accepts.
    pub fn sign_as(
        &self,
        label: &str,
        image: &Path,
        assertion: &Assertion,
        output: &Path,
    ) -> Result<()> {
        let definition = json!({
            "claim_generator_info": [{"name": "provenance", "version": env!("CARGO_PKG_VERSION")}],
            "assertions": [{
                "label": "c2pa.actions",
                "data": {"actions": actions(assertion.turn)},
            }],
        });
        let mut builder =
            Builder::from_context(c2pa::Context::new()).with_definition(definition)?;
        builder.add_assertion(label, assertion)?;
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

/// The C2PA actions that describe the edit, for readers that do not know
/// this assertion.
fn actions(turn: Turn) -> Vec<serde_json::Value> {
    let mut actions = vec![
        json!({"action": "c2pa.created", "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/digitalCapture"}),
        json!({"action": "c2pa.cropped"}),
    ];
    if turn != Turn::NONE {
        actions.push(json!({"action": "c2pa.orientation"}));
    }
    actions
}

/// The assertion in a signed file, once C2PA validation has passed.
pub fn read(signed: &Path) -> Result<Assertion> {
    Ok(read_labeled(signed)?.0)
}

/// The assertion and the label it was found under.
pub fn read_labeled(signed: &Path) -> Result<(Assertion, &'static str)> {
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
    let labels: Vec<&'static str> = manifest
        .assertions()
        .iter()
        .filter_map(|assertion| {
            [LABEL, LEGACY_LABEL]
                .into_iter()
                .find(|label| *label == assertion.label())
        })
        .collect();
    let [label] = labels[..] else {
        bail!("expected one {LABEL} assertion, found {}", labels.len());
    };
    let assertion: Assertion = manifest.find_assertion(label)?;
    ensure!(
        assertion.version == VERSION
            || (assertion.version == NO_TURN_VERSION && assertion.turn == Turn::NONE),
        "the {label} assertion is version {}, and this reader only reads versions {NO_TURN_VERSION} and {VERSION} (full-frame originals and crop proofs bound to their statement)",
        assertion.version
    );
    Ok((assertion, label))
}
