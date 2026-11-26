//! The commitment key as the prover reads it: one Lagrange basis per number
//! of variables.

use std::{borrow::Cow, ops::Range};

use ark_bls12_381::{Bls12_381, G1Affine};
use subroutines::pcs::prelude::MultilinearProverParam;

pub(crate) enum Srs {
    /// Deserialized into memory.
    Loaded(MultilinearProverParam<Bls12_381>),
}

impl Srs {
    /// The largest number of variables a polynomial may have.
    pub(crate) fn num_vars(&self) -> usize {
        match self {
            Srs::Loaded(params) => params.num_vars,
        }
    }

    /// The `2^num_vars` points that commit to a polynomial over `num_vars`
    /// variables. Opening a polynomial over n variables uses the bases for
    /// n - 1 down to 0 variables, one per round.
    pub(crate) fn basis(&self, num_vars: usize) -> Basis<'_> {
        assert!(num_vars <= self.num_vars(), "no basis this large");
        match self {
            Srs::Loaded(params) => {
                Basis::Points(&params.powers_of_g[params.num_vars - num_vars].evals)
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Basis<'a> {
    Points(&'a [G1Affine]),
}

impl<'a> Basis<'a> {
    pub(crate) fn len(&self) -> usize {
        match self {
            Basis::Points(points) => points.len(),
        }
    }

    /// The points of `range`, ready for arithmetic.
    pub(crate) fn points(&self, range: Range<usize>) -> Cow<'a, [G1Affine]> {
        match self {
            Basis::Points(points) => Cow::Borrowed(&points[range]),
        }
    }
}
