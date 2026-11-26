//! A prover for the same proof that keeps few large vectors alive. Every
//! function here writes the transcript exactly as the hyperplonk prover it
//! replaces, so the proof bytes do not change; the tests compare the two.

pub(crate) mod dense;
pub(crate) mod eq;
pub(crate) mod prover;
pub(crate) mod sumcheck;
