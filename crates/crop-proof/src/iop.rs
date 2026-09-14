//! Prover-side IOPs, and the public tables and points both sides rebuild.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use arithmetic::{merge_polynomials, VirtualPolynomial};
use ark_bls12_381::Bls12_381;
use ark_ff::{One, Zero};
use ark_poly::DenseMultilinearExtension;
use subroutines::{
    pcs::prelude::MultilinearProverParam,
    poly_iop::{
        prelude::{ProductCheck, SumCheck},
        PolyIOP,
    },
};
use transcript::IOPTranscript;

use crate::{fingerprint, Pcs, CROP, F, NUM_VARS, ORIGINAL};

pub(crate) type Poly = Arc<DenseMultilinearExtension<F>>;
pub(crate) type SumCheckProof = <PolyIOP<F> as SumCheck<F>>::SumCheckProof;
pub(crate) type ProductCheckProof = <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::ProductCheckProof;

/// Irreducible polynomial over GF(2) for each degree. The range check walks
/// GF(2^n) by repeated multiplication by x to lay out its tables.
pub(crate) const IRREDUCIBLE: [u64; 27] = [
    0, 0, 7, 11, 19, 37, 67, 131, 285, 529, 1033, 2053, 4179, 8219, 16707, 32771, 69643, 131081,
    262273, 524327, 1048585, 2097157, 4194307, 8388641, 16777351, 33554441, 67108935,
];

const MAX_PIXEL: u32 = 255;

pub(crate) fn check<T, E: std::fmt::Debug>(
    result: std::result::Result<T, E>,
    step: &str,
) -> Result<T> {
    result.map_err(|error| anyhow!("{step}: {error:?}"))
}

/// The multilinear extension of `values`, zero-padded to a power of two.
pub(crate) fn poly(mut values: Vec<F>) -> Poly {
    let num_vars = values.len().next_power_of_two().trailing_zeros() as usize;
    values.resize(1 << num_vars, F::zero());
    Arc::new(DenseMultilinearExtension::from_evaluations_vec(
        num_vars, values,
    ))
}

pub(crate) fn field_values(pixels: &[u8]) -> Vec<F> {
    pixels.iter().map(|&pixel| F::from(pixel)).collect()
}

pub(crate) fn challenge(transcript: &mut IOPTranscript<F>, label: &'static [u8]) -> Result<F> {
    check(
        transcript.get_and_append_challenge(label),
        "deriving a challenge",
    )
}

/// 128 row weights r and `r^T A`, which turn the fingerprint check into one
/// sumcheck per channel.
pub(crate) fn hash_weights(transcript: &mut IOPTranscript<F>) -> Result<(Vec<F>, Vec<F>)> {
    let weights = (0..fingerprint::HASH_ROWS)
        .map(|_| challenge(transcript, b"alpha"))
        .collect::<Result<Vec<_>>>()?;
    let combined = fingerprint::combine_rows(&weights);
    Ok((weights, combined))
}

/// `S^T r` for the crop selection S: original pixel (x, y) maps to published
/// pixel (x, y) when x is in the left half.
pub(crate) fn crop_weights(r: &[F]) -> Vec<F> {
    let mut weights = vec![F::zero(); ORIGINAL.pixels()];
    for y in 0..CROP.height {
        for x in 0..CROP.width {
            weights[y * ORIGINAL.width + x] = r[y * CROP.width + x];
        }
    }
    weights
}

fn times_x(slot: u64, size: u64, reduction: u64) -> u64 {
    let mut next = slot << 1;
    if next & size != 0 {
        next ^= reduction;
    }
    next & (size - 1)
}

/// The lookup table T (slot g^i holds i for i in 1..=255) and its shift
/// (slot g^(i+1) holds i), over NUM_VARS variables.
pub(crate) fn range_tables() -> (Vec<F>, Vec<F>) {
    let size = 1u64 << NUM_VARS;
    let reduction = IRREDUCIBLE[NUM_VARS] - size;
    let mut table = vec![F::zero(); size as usize];
    let mut shifted = vec![F::zero(); size as usize];
    let mut slot = 1;
    for value in 1..=MAX_PIXEL {
        table[slot as usize] = F::from(value);
        slot = times_x(slot, size, reduction);
        shifted[slot as usize] = F::from(value);
    }
    (table, shifted)
}

/// Every point opened at the end of the proof. Indices:
/// 0..3 hash sumcheck points; 3 the zero point and 4 the product-check final
/// query, both over NUM_VARS + 1 variables; 5 + 6i..11 + 6i the six points
/// derived from range check i; 23..26 crop sumcheck points.
pub(crate) struct OpeningPoints {
    pub points: Vec<Vec<F>>,
    /// First coordinate of each range check's point.
    pub range_starts: [F; 3],
}

pub(crate) fn opening_points(hash: [&[F]; 3], range: [&[F]; 3], crop: [&[F]; 3]) -> OpeningPoints {
    let range_vars = NUM_VARS + 1;
    let mut points: Vec<Vec<F>> = hash.iter().map(|point| point.to_vec()).collect();
    points.push(vec![F::zero(); range_vars]);
    let mut final_query = vec![F::one(); range_vars];
    final_query[0] = F::zero();
    points.push(final_query);

    let reduction = IRREDUCIBLE[range_vars] - (1 << range_vars);
    let mut range_starts = [F::zero(); 3];
    for (i, r) in range.iter().enumerate() {
        let without_last = &r[..r.len() - 1];
        let mut fiddled = Vec::with_capacity(range_vars);
        let mut shifted = Vec::with_capacity(range_vars);
        for (bit, &coordinate) in r.iter().enumerate().take(range_vars).skip(1) {
            shifted.push(coordinate);
            fiddled.push(if reduction >> bit & 1 == 1 {
                F::one() - coordinate
            } else {
                coordinate
            });
        }
        shifted.push(F::zero());
        fiddled.push(F::one());
        range_starts[i] = r[0];

        let prefixed = |first: F| [&[first], without_last].concat();
        points.extend([
            without_last.to_vec(),
            fiddled,
            shifted,
            r.to_vec(),
            prefixed(F::zero()),
            prefixed(F::one()),
        ]);
    }
    points.extend(crop.iter().map(|point| point.to_vec()));
    OpeningPoints {
        points,
        range_starts,
    }
}

pub(crate) struct Opening {
    pub commitment: usize,
    pub point: usize,
    /// Set when the evaluation is fixed by the protocol rather than read from the proof.
    pub fixed: Option<F>,
}

/// Openings of the image channels: at the hash point, at range check point
/// `without_last`, and at the crop point.
pub(crate) fn image_openings() -> Vec<Opening> {
    (0..3)
        .flat_map(|i| {
            [i, 5 + 6 * i, 23 + i].map(|point| Opening {
                commitment: i,
                point,
                fixed: None,
            })
        })
        .collect()
}

/// Openings of each range check's counts, product and fraction polynomials.
pub(crate) fn range_openings() -> Vec<Opening> {
    (0..3)
        .flat_map(|i| {
            let (counts, product, fraction) = (3 + 3 * i, 4 + 3 * i, 5 + 3 * i);
            let base = 5 + 6 * i;
            let open = |commitment, point, fixed| Opening {
                commitment,
                point,
                fixed,
            };
            [
                open(counts, 3, Some(F::zero())),
                open(counts, base + 3, None),
                open(counts, base + 1, None),
                open(counts, base + 2, None),
                open(product, 4, Some(F::one())),
                open(product, base + 3, None),
                open(fraction, base + 3, None),
                open(product, base + 4, None),
                open(fraction, base + 4, None),
                open(product, base + 5, None),
                open(fraction, base + 5, None),
            ]
        })
        .collect()
}

pub(crate) fn prove_products(
    weights: &Poly,
    channels: &[Poly; 3],
    transcript: &mut IOPTranscript<F>,
) -> Result<[SumCheckProof; 3]> {
    let polynomials = channels
        .iter()
        .map(|channel| {
            let mut product = VirtualPolynomial::new_from_mle(weights, F::one());
            check(
                product.mul_by_mle(channel.clone(), F::one()),
                "building a sumcheck polynomial",
            )?;
            Ok(product)
        })
        .collect::<Result<Vec<_>>>()?;
    let proofs = polynomials
        .iter()
        .map(|polynomial| {
            check(
                <PolyIOP<F> as SumCheck<F>>::prove(polynomial, transcript),
                "sumcheck",
            )
        })
        .collect::<Result<Vec<_>>>()?;
    proofs
        .try_into()
        .map_err(|_| anyhow!("expected three sumchecks"))
}

pub(crate) struct RangeCheck {
    pub proof: ProductCheckProof,
    pub counts: Poly,
    pub product: Poly,
    pub fraction: Poly,
}

/// HyperPlonk lookup proving every channel value is in 0..=255: the multiset
/// of (value, T) and (value, T shifted) pairs equals the sorted counts H and
/// H shifted, where H repeats each table value once per occurrence plus one.
pub(crate) fn prove_range(
    channel: &Poly,
    pixels: &[u8],
    params: &MultilinearProverParam<Bls12_381>,
    transcript: &mut IOPTranscript<F>,
) -> Result<RangeCheck> {
    let (table, shifted_table) = range_tables();
    let (counts, shifted_counts) = sorted_counts(pixels);
    let left = [
        check(
            merge_polynomials(&[channel.clone(), poly(table)]),
            "merging the range table",
        )?,
        check(
            merge_polynomials(&[channel.clone(), poly(shifted_table)]),
            "merging the range table",
        )?,
    ];
    let right = [poly(counts), poly(shifted_counts)];

    let offset = challenge(transcript, b"alpha")?;
    let constant = DenseMultilinearExtension::from_evaluations_vec(
        NUM_VARS + 1,
        vec![offset; 1 << (NUM_VARS + 1)],
    );
    let mix = challenge(transcript, b"alpha")?;
    let f = &(left[0].as_ref() + &constant) + &scaled(&left[1], mix);
    let g = &(right[0].as_ref() + &constant) + &scaled(&right[1], mix);
    let (proof, product, fraction) = check(
        <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::prove(
            params,
            &[Arc::new(f)],
            &[Arc::new(g)],
            transcript,
        ),
        "range product check",
    )?;
    Ok(RangeCheck {
        proof,
        counts: right[0].clone(),
        product,
        fraction,
    })
}

fn scaled(polynomial: &Poly, by: F) -> DenseMultilinearExtension<F> {
    let values = polynomial
        .evaluations
        .iter()
        .map(|value| *value * by)
        .collect();
    DenseMultilinearExtension::from_evaluations_vec(polynomial.num_vars, values)
}

fn sorted_counts(pixels: &[u8]) -> (Vec<F>, Vec<F>) {
    let size = 1u64 << (NUM_VARS + 1);
    let reduction = IRREDUCIBLE[NUM_VARS + 1] - size;
    let mut occurrences = [0usize; MAX_PIXEL as usize + 1];
    for &pixel in pixels {
        occurrences[pixel as usize] += 1;
    }
    let mut counts = vec![F::zero(); size as usize];
    let mut shifted = vec![F::zero(); size as usize];
    let mut slot = 1;
    for (value, &occurrence) in occurrences.iter().enumerate() {
        for _ in 0..=occurrence {
            counts[slot as usize] = F::from(value as u32);
            slot = times_x(slot, size, reduction);
            shifted[slot as usize] = F::from(value as u32);
        }
    }
    (counts, shifted)
}
