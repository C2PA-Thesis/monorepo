//! The reader checks. Each one first says what it is about to establish,
//! then reports its outcome, so a reader can follow the argument as it runs.

use std::{path::Path, time::Instant};

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use crop_proof::{Rect, VerifierParams};
use serde::Serialize;

use crate::{
    image,
    location::LocationTool,
    manifest::{self, LABEL},
    receipt::{short, TrustedKeys},
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

/// What a check is about to establish, and how it ended.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    Checking {
        check: Check,
        intent: String,
    },
    Passed {
        check: Check,
        intent: String,
        outcome: String,
        seconds: f64,
    },
    Rejected {
        check: Check,
        intent: String,
        reason: String,
        seconds: f64,
    },
}

#[derive(Debug, Serialize)]
pub struct Step {
    pub check: Check,
    pub intent: String,
    /// The outcome when the check passed, the reason when it rejected the file.
    pub detail: String,
}

/// The joint statement an accepted file supports: these pixels are the `crop`
/// of an original that `device` signed together with a coordinate in `cell`.
/// The two proofs meet only in the receipt, through `fingerprint` and `envelope`.
#[derive(Debug, Serialize)]
pub struct Claim {
    /// Key id of the device that signed the receipt.
    pub device: String,
    /// Asserted by the device, not by a trusted clock.
    pub captured_at: String,
    /// SHA-256 of the signed fingerprint's JSON. It only names the fingerprint on
    /// screen; the crop proof checks the fingerprint itself.
    pub fingerprint: String,
    /// The signed envelope, which the location proof ties to the cell.
    pub envelope: String,
    pub cell: String,
    pub crop: Rect,
}

#[derive(Debug, Default, Serialize)]
pub struct Verdict {
    pub passed: Vec<Step>,
    /// The first check that failed; later checks do not run.
    pub rejected: Option<Step>,
    /// Set only when every check passed.
    pub claim: Option<Claim>,
}

impl Verdict {
    fn check<T>(
        &mut self,
        on: &mut dyn FnMut(Event),
        check: Check,
        intent: String,
        run: impl FnOnce() -> Result<(T, String)>,
    ) -> Option<T> {
        on(Event::Checking {
            check,
            intent: intent.clone(),
        });
        let started = Instant::now();
        let result = run();
        let seconds = started.elapsed().as_secs_f64();
        match result {
            Ok((value, outcome)) => {
                on(Event::Passed {
                    check,
                    intent: intent.clone(),
                    outcome: outcome.clone(),
                    seconds,
                });
                self.passed.push(Step {
                    check,
                    intent,
                    detail: outcome,
                });
                Some(value)
            }
            Err(error) => {
                let reason = format!("{error:#}");
                on(Event::Rejected {
                    check,
                    intent: intent.clone(),
                    reason: reason.clone(),
                    seconds,
                });
                self.rejected = Some(Step {
                    check,
                    intent,
                    detail: reason,
                });
                None
            }
        }
    }
}

/// A check after the manifest: what it establishes, and how to run it.
type Later<'a> = (Check, String, &'a dyn Fn() -> Result<String>);

/// What a reader needs: the trusted device keys and both proof verifiers.
pub struct Verifier {
    pub trusted: TrustedKeys,
    pub crop_params: VerifierParams,
    pub location: LocationTool,
}

impl Verifier {
    /// `cell`, when given, is the region the reader asks about. `on` sees
    /// every check start and end.
    pub fn verify(&self, signed: &Path, cell: Option<&str>, on: &mut dyn FnMut(Event)) -> Verdict {
        let mut verdict = Verdict::default();
        let Some(assertion) = verdict.check(
            on,
            Check::Manifest,
            "reading the C2PA manifest and validating its claim signature".to_string(),
            || {
                Ok((
                    manifest::read(signed)?,
                    format!("valid, with one {LABEL} assertion"),
                ))
            },
        ) else {
            return verdict;
        };
        let receipt = &assertion.receipt;
        let fingerprint = receipt.fingerprint_digest();

        let steps: [Later; 3] = [
            (
                Check::Device,
                format!(
                    "checking the device signature over fingerprint {} and envelope {}",
                    short(&fingerprint),
                    short(&receipt.envelope)
                ),
                &|| {
                    receipt.verify(&self.trusted)?;
                    Ok(format!(
                        "signed by trusted device {} at {}",
                        short(&receipt.device),
                        receipt.captured_at
                    ))
                },
            ),
            (
                Check::Image,
                format!(
                    "verifying that the published pixels are the {} of the original with fingerprint {}",
                    assertion.crop,
                    short(&fingerprint)
                ),
                &|| {
                    let published = image::read_png(signed)?;
                    let proof = BASE64
                        .decode(&assertion.image_proof)
                        .context("the crop proof is not base64")?;
                    crop_proof::verify(
                        &self.crop_params,
                        &published,
                        assertion.crop,
                        &receipt.fingerprint,
                        &proof,
                    )
                    .context("the proof does not tie these pixels to the signed fingerprint")?;
                    Ok("proof accepted".to_string())
                },
            ),
            (
                Check::Location,
                format!(
                    "verifying that the coordinate in envelope {} lies in cell {}",
                    short(&receipt.envelope),
                    assertion.cell
                ),
                &|| {
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
                    Ok("proof accepted".to_string())
                },
            ),
        ];
        for (check, intent, run) in steps {
            if verdict
                .check(on, check, intent, || Ok(((), run()?)))
                .is_none()
            {
                return verdict;
            }
        }
        verdict.claim = Some(Claim {
            device: receipt.device.clone(),
            captured_at: receipt.captured_at.clone(),
            fingerprint,
            envelope: receipt.envelope.clone(),
            cell: assertion.cell.clone(),
            crop: assertion.crop,
        });
        verdict
    }
}
