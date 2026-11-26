//! A prover for the same proof that keeps few large vectors alive. Every
//! function here writes the transcript exactly as the hyperplonk prover it
//! replaces, so the proof bytes do not change; the tests compare the two.

pub(crate) mod dense;
pub(crate) mod eq;
#[allow(dead_code)] // until the prover commits through it
pub(crate) mod msm;
pub(crate) mod prover;
#[allow(dead_code)] // until the prover commits through it
pub(crate) mod srs;
pub(crate) mod sumcheck;

use ark_ff::Zero;

use crate::F;

/// The values of a polynomial over the hypercube, as the prover holds them.
/// Pixels and range counts stay bytes. A slice shorter than the hypercube
/// stands for its values followed by zeros.
#[allow(dead_code)] // until the prover keeps bytes
#[derive(Clone, Copy)]
pub(crate) enum Values<'a> {
    Bytes(&'a [u8]),
    Field(&'a [F]),
}

#[allow(dead_code)]
impl Values<'_> {
    /// How many values are stored; the rest are zero.
    pub(crate) fn len(&self) -> usize {
        match self {
            Values::Bytes(values) => values.len(),
            Values::Field(values) => values.len(),
        }
    }

    pub(crate) fn get(&self, index: usize) -> F {
        match self {
            Values::Bytes(values) => values.get(index).map_or(F::zero(), |&value| F::from(value)),
            Values::Field(values) => values.get(index).copied().unwrap_or(F::zero()),
        }
    }
}
