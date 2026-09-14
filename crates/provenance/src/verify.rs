use std::path::Path;

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::VerifierParams;
use p256::ecdsa::VerifyingKey;
use serde::Serialize;

use crate::{
    capture::{self, short},
    location::LocationTool,
    manifest::{self, LABEL},
};

/// Reader checks, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Check {
    Manifest,
    Device,
    Image,
    Location,
}

impl Check {
    pub const ALL: [Check; 4] = [
        Check::Manifest,
        Check::Device,
        Check::Image,
        Check::Location,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Check::Manifest => "C2PA manifest",
            Check::Device => "device receipt",
            Check::Image => "crop proof",
            Check::Location => "location proof",
        }
    }

    /// Exit status of `provenance verify` when this check rejects the file.
    pub fn exit_code(self) -> u8 {
        10 + self as u8
    }
}

#[derive(Debug, Serialize)]
pub struct Step {
    pub check: Check,
    pub detail: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Verdict {
    pub passed: Vec<Step>,
    /// The first check that failed; later checks do not run.
    pub rejected: Option<Step>,
}

impl Verdict {
    fn check<T>(&mut self, check: Check, run: impl FnOnce() -> Result<(T, String)>) -> Option<T> {
        match run() {
            Ok((value, detail)) => {
                self.passed.push(Step { check, detail });
                Some(value)
            }
            Err(error) => {
                self.rejected = Some(Step {
                    check,
                    detail: format!("{error:#}"),
                });
                None
            }
        }
    }
}

/// What a reader needs: the trusted device key and both proof verifiers.
pub struct Verifier {
    pub device: VerifyingKey,
    pub crop_params: VerifierParams,
    pub location: LocationTool,
}

impl Verifier {
    /// `cell`, when given, is the region the reader asks about.
    pub fn verify(&self, signed: &Path, cell: Option<&str>) -> Verdict {
        let mut verdict = Verdict::default();
        let Some(assertion) = verdict.check(Check::Manifest, || {
            Ok((
                manifest::read(signed)?,
                format!("valid, with one {LABEL} assertion"),
            ))
        }) else {
            return verdict;
        };
        let receipt = &assertion.receipt;

        let steps: [(Check, &dyn Fn() -> Result<String>); 3] = [
            (Check::Device, &|| {
                receipt.verify(&self.device)?;
                Ok(format!(
                    "signed by device {} at {}",
                    short(&receipt.device),
                    receipt.captured_at
                ))
            }),
            (Check::Image, &|| {
                let published = capture::read_png(signed)?;
                let proof = BASE64
                    .decode(&assertion.image_proof)
                    .context("the crop proof is not base64")?;
                crop_proof::verify(&self.crop_params, &published, &receipt.fingerprint, &proof)
                    .context("the proof does not tie these pixels to the signed fingerprint")?;
                Ok("the published pixels are the left half of the signed original".to_string())
            }),
            (Check::Location, &|| {
                if let Some(cell) = cell {
                    ensure!(
                        cell == assertion.cell,
                        "the file claims cell {}, not {cell}",
                        assertion.cell
                    );
                }
                self.location.verify(
                    &assertion.cell,
                    &receipt.envelope,
                    &assertion.location_proof,
                )?;
                Ok(format!("the signed envelope is in cell {}", assertion.cell))
            }),
        ];
        for (check, run) in steps {
            if verdict.check(check, || Ok(((), run()?))).is_none() {
                break;
            }
        }
        verdict
    }
}
