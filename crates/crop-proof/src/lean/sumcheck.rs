//! The sumcheck transcript, apart from how a round's polynomial is computed.

use std::marker::PhantomData;

use anyhow::{ensure, Result};
use arithmetic::VPAuxInfo;
use transcript::IOPTranscript;

use crate::{
    iop::{challenge, check},
    wire::Sumcheck,
    F,
};

/// The prover's side of one sumcheck: a polynomial over `num_vars` variables
/// whose degree in each is at most `max_degree`.
pub(super) trait Rounds {
    fn num_vars(&self) -> usize;

    fn max_degree(&self) -> usize;

    /// Fixes the previous round's variable to `challenge`, which is `None` in
    /// the first round only, and returns this round's polynomial at
    /// `0..=max_degree`. Variables are fixed from the lowest index bit up.
    fn round(&mut self, challenge: Option<F>) -> Vec<F>;
}

/// Runs the sumcheck as `SumCheck::prove` of hyperplonk does, byte for byte.
pub(super) fn prove(
    rounds: &mut impl Rounds,
    transcript: &mut IOPTranscript<F>,
) -> Result<Sumcheck> {
    let num_variables = rounds.num_vars();
    ensure!(num_variables > 0, "a sumcheck needs at least one variable");
    let max_degree = rounds.max_degree();
    check(
        transcript.append_serializable_element(
            b"aux info",
            &VPAuxInfo::<F> {
                max_degree,
                num_variables,
                phantom: PhantomData,
            },
        ),
        "appending to the transcript",
    )?;
    let mut point = Vec::with_capacity(num_variables);
    let mut messages = Vec::with_capacity(num_variables);
    for _ in 0..num_variables {
        let evaluations = rounds.round(point.last().copied());
        ensure!(
            evaluations.len() == max_degree + 1,
            "a sumcheck round returned {} evaluations for degree {max_degree}",
            evaluations.len()
        );
        // The message hyperplonk appends is a struct holding only this vector.
        check(
            transcript.append_serializable_element(b"prover msg", &evaluations),
            "appending to the transcript",
        )?;
        messages.push(evaluations);
        point.push(challenge(transcript, b"Internal round")?);
    }
    Ok(Sumcheck {
        point,
        rounds: messages,
    })
}
