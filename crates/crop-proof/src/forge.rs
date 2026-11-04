//! Security analysis: forgers for attacks A2 and A3.
//!
//! A2 (`substitute_crop`): the crop challenges are drawn before the published
//! image is absorbed, so anyone can replay them from the public proof and find
//! another image with the same claim.
//!
//! A3 (`forge_crop_proof`): a dishonest prover for the range-check bug.
//! The range check draws its challenges `offset` and `mix` before the sorted
//! vector H is committed, so a prover can pick H after seeing them and
//! satisfy the product check for channel values outside 0..=255. Without that
//! bound, the fingerprint `A x = h` has solutions with huge entries, and any
//! published crop can be proven against an honest fingerprint.
//!
//! Only built with the `attack-hooks` feature. The honest prover is untouched.

use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use arithmetic::merge_polynomials;
use ark_bls12_381::Bls12_381;
use ark_ff::{BigInteger, Field, PrimeField, UniformRand, Zero};
use ark_poly::DenseMultilinearExtension;
use fingerprint::{self, HASH_ROWS};
use num_bigint::{BigInt, Sign};
use subroutines::{
    pcs::prelude::{MultilinearProverParam, PolynomialCommitmentScheme},
    poly_iop::{prelude::ProductCheck, PolyIOP},
};
use transcript::IOPTranscript;

use crate::{
    crop::{append, new_transcript, open},
    hooks,
    iop::{self, challenge, check, poly, OpeningPoints, Poly, RangeCheck, IRREDUCIBLE},
    lattice,
    wire::{self, CropProof, Sumcheck},
    Fingerprint, Pcs, ProverParams, RgbImage, CROP, F, NUM_VARS, ORIGINAL,
};

/// Published pixels that absorb the difference in A2, one per lattice
/// dimension. With 64 of them a solution of size about p^(1/64), or 2^4, exists.
fn corrective_pixels(attempt: usize) -> Vec<usize> {
    (0..64).map(|j| 97 * j + 13 + 7 * attempt).collect()
}

/// A solution is short but not always within 0..=255 around mid-gray; moving
/// the corrective pixels gives a new lattice and usually a shorter solution.
const ATTEMPTS: usize = 8;

/// Returns `target` with a few pixels per channel adjusted so that `verify`
/// accepts it with the honest `proof` and `fingerprint`, unchanged. Needs only
/// what a file publishes: the honest crop, the fingerprint and the proof.
pub fn substitute_crop(
    fingerprint: &Fingerprint,
    proof: &[u8],
    published: &RgbImage,
    target: &RgbImage,
) -> Result<RgbImage> {
    ensure!(
        published.size() == CROP,
        "the published image must be {CROP}"
    );
    ensure!(target.size() == CROP, "the target must be {CROP}");
    let challenges = hooks::crop_challenges(fingerprint, proof)?;
    let mut channels = Vec::with_capacity(3);
    for (honest, target) in published.channels().iter().zip(target.channels()) {
        let fitted = (0..ATTEMPTS)
            .map(|attempt| fit_channel(&challenges, honest, target, attempt))
            .find_map(|fitted| fitted.transpose())
            .transpose()?
            .with_context(|| format!("no correction within 0..=255 in {ATTEMPTS} attempts"))?;
        channels.push(fitted);
    }
    RgbImage::new(CROP, channels.try_into().expect("three channels"))
}

/// `target` with the corrective pixels of `attempt` set so that its claim
/// equals the honest one, or `None` when a correction leaves 0..=255.
fn fit_channel(
    challenges: &[F],
    honest: &[u8],
    target: &[u8],
    attempt: usize,
) -> Result<Option<Vec<u8>>> {
    let positions = corrective_pixels(attempt);
    let coefficients: Vec<BigInt> = positions
        .iter()
        .map(|&i| to_bigint(&challenges[i]))
        .collect();
    let modulus = BigInt::from_bytes_be(Sign::Plus, &F::MODULUS.to_bytes_be());

    let mut channel = target.to_vec();
    // Mid-gray leaves room for a signed correction on either side.
    for &i in &positions {
        channel[i] = 128;
    }
    let gap = claim(challenges, honest) - claim(challenges, &channel);
    let deltas = lattice::solve_congruence(&coefficients, &to_bigint(&gap), &modulus)
        .context("no short lattice solution")?;
    for (&i, delta) in positions.iter().zip(&deltas) {
        match u8::try_from(BigInt::from(128) + delta) {
            Ok(value) => channel[i] = value,
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(channel))
}

fn claim(challenges: &[F], pixels: &[u8]) -> F {
    challenges
        .iter()
        .zip(pixels)
        .map(|(challenge, &pixel)| *challenge * F::from(pixel))
        .sum()
}

fn to_bigint(value: &F) -> BigInt {
    BigInt::from_bytes_be(Sign::Plus, &value.into_bigint().to_bytes_be())
}

/// Original pixels the forger solves for, so that `A x = h` holds exactly.
/// They sit in the right half, which the published crop never shows.
fn free_pixels() -> impl Iterator<Item = usize> {
    (0..HASH_ROWS).map(|k| CROP.width + k)
}

/// Builds a crop proof that `target` is the left half of the original behind
/// `fingerprint`, without knowing that original. `base` only fills the right
/// half; any image of the right size works, so the published fingerprint is
/// all the forger needs.
pub fn forge_crop_proof(
    params: &ProverParams,
    fingerprint: &Fingerprint,
    base: &RgbImage,
    target: &RgbImage,
) -> Result<Vec<u8>> {
    ensure!(base.size() == ORIGINAL, "the base must be {ORIGINAL}");
    ensure!(target.size() == CROP, "the target must be {CROP}");
    let hashes = fingerprint.elements()?;
    let mut channels = Vec::with_capacity(3);
    for ((base, target), hash) in base.channels().iter().zip(target.channels()).zip(&hashes) {
        channels.push(forged_channel(base, target, hash)?);
    }
    let channels: [Vec<F>; 3] = channels.try_into().expect("three channels");
    wire::encode(&prove_forged(&params.pcs, channels)?)
}

/// Field values whose fingerprint is `hash` and whose left half is `target`.
pub fn forged_channel(base: &[u8], target: &[u8], hash: &[F]) -> Result<Vec<F>> {
    let mut pixels = base.to_vec();
    for y in 0..CROP.height {
        for x in 0..CROP.width {
            pixels[y * ORIGINAL.width + x] = target[y * CROP.width + x];
        }
    }
    let free: Vec<usize> = free_pixels().collect();
    for &i in &free {
        pixels[i] = 0;
    }
    let partial = fingerprint::hash_channel(&pixels);

    // Columns of A at the free pixels. Row i of A is a ChaCha8 stream, so each
    // row is replayed up to the last free pixel.
    let last = *free.iter().max().expect("free pixels");
    let mut columns = vec![vec![F::zero(); free.len()]; HASH_ROWS];
    for (row, entries) in columns.iter_mut().enumerate() {
        let mut stream = fingerprint::matrix_row(row);
        let prefix: Vec<F> = (0..=last).map(|_| F::rand(&mut stream)).collect();
        for (k, &i) in free.iter().enumerate() {
            entries[k] = prefix[i];
        }
    }
    let wanted: Vec<F> = hash.iter().zip(&partial).map(|(h, p)| *h - p).collect();
    let solution = solve(columns, wanted)?;

    let mut values = iop::field_values(&pixels);
    for (k, &i) in free.iter().enumerate() {
        values[i] = solution[k];
    }
    Ok(values)
}

/// Gaussian elimination over F for a square system.
pub fn solve(mut matrix: Vec<Vec<F>>, mut rhs: Vec<F>) -> Result<Vec<F>> {
    let n = rhs.len();
    for col in 0..n {
        let Some(pivot) = (col..n).find(|&row| !matrix[row][col].is_zero()) else {
            bail!("singular system");
        };
        matrix.swap(col, pivot);
        rhs.swap(col, pivot);
        let inverse = matrix[col][col].inverse().expect("nonzero pivot");
        for value in &mut matrix[col][col..] {
            *value *= inverse;
        }
        rhs[col] *= inverse;
        for row in 0..n {
            if row != col && !matrix[row][col].is_zero() {
                let factor = matrix[row][col];
                let pivot_row = matrix[col].clone();
                for (value, pivot_value) in matrix[row][col..].iter_mut().zip(&pivot_row[col..]) {
                    *value -= factor * pivot_value;
                }
                let pivot_rhs = rhs[col];
                rhs[row] -= factor * pivot_rhs;
            }
        }
    }
    Ok(rhs)
}

/// The honest prover's steps, except for the range check.
fn prove_forged(pcs: &MultilinearProverParam<Bls12_381>, values: [Vec<F>; 3]) -> Result<CropProof> {
    let channels: [Poly; 3] = values.map(poly);
    let mut commitments = channels
        .iter()
        .map(|channel| check(Pcs::commit(pcs, channel), "committing to the image"))
        .collect::<Result<Vec<_>>>()?;
    let mut transcript = new_transcript();
    for commitment in &commitments {
        append(&mut transcript, b"img(x)", commitment)?;
    }

    let (_, combined_rows) = iop::hash_weights(&mut transcript)?;
    let hash = iop::prove_products(&poly(combined_rows), &channels, &mut transcript)?;
    let mut ranges = Vec::with_capacity(3);
    for channel in &channels {
        ranges.push(forged_range(channel, pcs, &mut transcript)?);
    }
    let crop_challenges = check(
        transcript.get_and_append_challenge_vectors(b"frievald", CROP.pixels()),
        "deriving crop challenges",
    )?;
    let crop = iop::prove_products(
        &poly(iop::crop_weights(&crop_challenges)),
        &channels,
        &mut transcript,
    )?;

    for range in &ranges {
        let counts = check(
            Pcs::commit(pcs, &range.counts),
            "committing to range counts",
        )?;
        append(&mut transcript, b"hCom(x)", &counts)?;
        commitments.extend([counts, range.proof.prod_x_comm, range.proof.frac_comm]);
    }

    let OpeningPoints { points, .. } = iop::opening_points(
        hash.each_ref().map(|sumcheck| sumcheck.point.as_slice()),
        std::array::from_fn(|i| ranges[i].proof.zero_check_proof.point.as_slice()),
        crop.each_ref().map(|sumcheck| sumcheck.point.as_slice()),
    );
    let polynomials: Vec<&Poly> = channels
        .iter()
        .chain(
            ranges
                .iter()
                .flat_map(|range| [&range.counts, &range.product, &range.fraction]),
        )
        .collect();
    let image_openings = open(
        pcs,
        &polynomials,
        &points,
        &iop::image_openings(),
        &mut transcript,
    )?;
    let range_openings = open(
        pcs,
        &polynomials,
        &points,
        &iop::range_openings(),
        &mut transcript,
    )?;

    let mut range_sumchecks = ranges
        .into_iter()
        .map(|range| Sumcheck::from(range.proof.zero_check_proof));
    Ok(CropProof {
        commitments,
        hash: hash.map(Sumcheck::from),
        range: std::array::from_fn(|_| range_sumchecks.next().expect("three range checks")),
        crop: crop.map(Sumcheck::from),
        image_openings,
        range_openings,
    })
}

/// The range check with H chosen after `offset` and `mix`, which the honest
/// verifier allows because H is absorbed into the transcript only later.
fn forged_range(
    channel: &Poly,
    pcs: &MultilinearProverParam<Bls12_381>,
    transcript: &mut IOPTranscript<F>,
) -> Result<RangeCheck> {
    let (table, shifted_table) = iop::range_tables();
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

    let offset = challenge(transcript, b"alpha")?;
    let mix = challenge(transcript, b"alpha")?;

    let target: F = left[0]
        .evaluations
        .iter()
        .zip(&left[1].evaluations)
        .map(|(a, b)| *a + offset + mix * b)
        .product();
    let (counts, shifted) = sorted_vector_for_product(target, offset, mix)?;

    let size = 1 << (NUM_VARS + 1);
    let constant =
        DenseMultilinearExtension::from_evaluations_vec(NUM_VARS + 1, vec![offset; size]);
    let right = [poly(counts), poly(shifted)];
    let f = &(left[0].as_ref() + &constant) + &scale(&left[1], mix);
    let g = &(right[0].as_ref() + &constant) + &scale(&right[1], mix);
    let (proof, product, fraction) = check(
        <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::prove(
            pcs,
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

/// A sorted vector H (`counts` in this crate), and H shifted by the generator
/// order of the lookup, such that
/// the product of `H + offset + mix * shifted` over all slots is `target`.
///
/// H is zero except at two slots. A nonzero slot j appears in two factors,
/// `H[j] + offset` at j and `offset + mix * H[j]` at the next slot, so fixing
/// one slot leaves a quadratic in the other. When its discriminant is not a
/// square the first slot takes another value.
pub fn sorted_vector_for_product(target: F, offset: F, mix: F) -> Result<(Vec<F>, Vec<F>)> {
    let vars = NUM_VARS + 1;
    let size = 1u64 << vars;
    let reduction = IRREDUCIBLE[vars] - size;
    // Slots 1 and 4 are nonzero and not adjacent in the generator order
    // (1 -> 2, 4 -> 8).
    let (first, second) = (4usize, 1usize);

    let rest = offset.pow([size - 4]);
    for tweak in 1u64..1000 {
        let u = F::from(tweak);
        let moved = (u + offset) * (offset + mix * u);
        // (t + offset)(offset + mix t) = target / (rest * moved)
        let k = target / (rest * moved);
        let (a, b, c) = (mix, offset + mix * offset, offset * offset - k);
        let Some(root) = (b * b - F::from(4u64) * a * c).sqrt() else {
            continue;
        };
        let t = (root - b) / (a + a);

        let mut counts = vec![F::zero(); size as usize];
        counts[first] = u;
        counts[second] = t;
        let mut shifted = vec![F::zero(); size as usize];
        for slot in 1..size {
            shifted[iop::times_x(slot, size, reduction) as usize] = counts[slot as usize];
        }
        let product: F = counts
            .iter()
            .zip(&shifted)
            .map(|(h, s)| *h + offset + mix * s)
            .product();
        ensure!(product == target, "the forged H misses the target");
        return Ok((counts, shifted));
    }
    bail!("no square discriminant in 1000 tries")
}

fn scale(polynomial: &Poly, by: F) -> DenseMultilinearExtension<F> {
    let values = polynomial.evaluations.iter().map(|v| *v * by).collect();
    DenseMultilinearExtension::from_evaluations_vec(polynomial.num_vars, values)
}
