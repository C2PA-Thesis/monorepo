//! Multi-scalar multiplications over a basis of the commitment key.

use ark_bls12_381::G1Projective;
use ark_ec::scalar_mul::variable_base::VariableBaseMSM;
use ark_ff::Zero;

use super::srs::Basis;
use crate::F;

/// `sum of scalars[i] * basis[i]`. Scalars past the end of the slice are zero.
pub(crate) fn msm_bytes(basis: &Basis, scalars: &[u8]) -> G1Projective {
    let scalars: Vec<F> = scalars.iter().map(|&scalar| F::from(scalar)).collect();
    msm(basis, &scalars)
}

/// `sum of scalars[i] * basis[i]`. Scalars past the end of the slice are zero.
pub(crate) fn msm(basis: &Basis, scalars: &[F]) -> G1Projective {
    msm_with(basis, scalars.len(), |start, out| {
        out.copy_from_slice(&scalars[start..start + out.len()])
    })
}

/// `sum of scalar(i) * basis[i]` for `i < len`, where `fill(start, out)`
/// writes the scalars of `start..start + out.len()`. It is called once per
/// index, in any order and from several threads.
pub(crate) fn msm_with(
    basis: &Basis,
    len: usize,
    fill: impl Fn(usize, &mut [F]) + Sync,
) -> G1Projective {
    assert!(len <= basis.len(), "more scalars than basis points");
    let mut scalars = vec![F::zero(); len];
    fill(0, &mut scalars);
    G1Projective::msm_unchecked(&basis.points(0..len), &scalars)
}
