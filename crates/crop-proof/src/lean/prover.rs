//! The crop prover. Same steps and transcript as the reference prover in
//! `crop.rs`, which the tests keep to compare proofs byte for byte.

use anyhow::{anyhow, Result};
use ark_bls12_381::Bls12_381;
use ark_ff::{batch_inversion, One, Zero};
use rayon::prelude::*;
use subroutines::pcs::prelude::{Commitment, MultilinearProverParam, PolynomialCommitmentScheme};
use transcript::IOPTranscript;

use super::{dense::Dense, eq, open, products, srs::Srs, sumcheck, Values};
use crate::{
    crop::{append, crop_challenges, new_transcript},
    iop::{self, challenge, check, image_poly, OpeningPoints, Poly},
    probe::mark,
    wire::{CropProof, Sumcheck},
    Pcs, Rect, RgbImage, Turn, F, NUM_VARS,
};

pub(crate) fn prove_core(
    srs: &Srs,
    original: &RgbImage,
    rect: Rect,
    turn: Turn,
    statement: &[u8; 32],
) -> Result<CropProof> {
    let Srs::Loaded(pcs) = srs;
    let size = original.size();
    mark("commit image");
    let channels: [Poly; 3] = original
        .channels()
        .each_ref()
        .map(|pixels| image_poly(iop::field_values(pixels)));
    let mut commitments = channels
        .iter()
        .map(|channel| check(Pcs::commit(pcs, channel), "committing to the image"))
        .collect::<Result<Vec<_>>>()?;
    let mut transcript = new_transcript(statement)?;
    for commitment in &commitments {
        append(&mut transcript, b"img(x)", commitment)?;
    }

    mark("hash weights");
    let (_, combined_rows) = iop::hash_weights(&mut transcript, size)?;
    mark("hash sumcheck");
    let pixels = original.channels().each_ref().map(Vec::as_slice);
    let hash = products::prove_products(NUM_VARS, &combined_rows, pixels, &mut transcript)?;
    drop(combined_rows);

    let tables = iop::range_tables();
    let mut ranges = Vec::with_capacity(3);
    const RANGE: [&str; 3] = ["range check 0", "range check 1", "range check 2"];
    for (i, (channel, pixels)) in channels.iter().zip(original.channels()).enumerate() {
        mark(RANGE[i]);
        ranges.push(prove_range(channel, pixels, &tables, pcs, &mut transcript)?);
    }
    drop(tables);

    mark("crop sumcheck");
    let weights = {
        let challenges = crop_challenges(&mut transcript, rect, turn)?;
        iop::crop_weights(size, rect, turn, &challenges)
    };
    let crop = products::prove_products(NUM_VARS, &weights, pixels, &mut transcript)?;
    drop(weights);

    for range in &ranges {
        commitments.extend([
            range.counts_commitment,
            range.product_commitment,
            range.fraction_commitment,
        ]);
    }

    let OpeningPoints { points, .. } = iop::opening_points(
        hash.each_ref().map(|sumcheck| sumcheck.point.as_slice()),
        std::array::from_fn(|i| ranges[i].sumcheck.point.as_slice()),
        crop.each_ref().map(|sumcheck| sumcheck.point.as_slice()),
    );
    let polynomials: Vec<Values> = pixels
        .iter()
        .map(|channel| Values::Bytes(channel))
        .chain(ranges.iter().flat_map(|range| {
            [&range.counts, &range.product, &range.fraction]
                .map(|polynomial| Values::Field(&polynomial.evaluations))
        }))
        .collect();
    mark("image opening");
    let image_openings = open::open(
        srs,
        NUM_VARS,
        &polynomials,
        &points,
        &iop::image_openings(),
        &mut transcript,
    )?;
    mark("range opening");
    let range_openings = open::open(
        srs,
        NUM_VARS + 1,
        &polynomials,
        &points,
        &iop::range_openings(),
        &mut transcript,
    )?;

    mark("encode");
    let mut range_sumchecks = ranges.into_iter().map(|range| range.sumcheck);
    Ok(CropProof {
        commitments,
        hash,
        range: std::array::from_fn(|_| range_sumchecks.next().expect("three range checks")),
        crop,
        image_openings,
        range_openings,
    })
}

struct RangeCheck {
    sumcheck: Sumcheck,
    counts: Poly,
    counts_commitment: Commitment<Bls12_381>,
    product: Poly,
    product_commitment: Commitment<Bls12_381>,
    fraction: Poly,
    fraction_commitment: Commitment<Bls12_381>,
}

/// The range check of `iop::prove_range`, with hyperplonk's product check
/// and zero check written out.
fn prove_range(
    channel: &Poly,
    pixels: &[u8],
    (table, shifted_table): &(Vec<F>, Vec<F>),
    pcs: &MultilinearProverParam<Bls12_381>,
    transcript: &mut IOPTranscript<F>,
) -> Result<RangeCheck> {
    let num_vars = NUM_VARS + 1;
    let half = 1 << NUM_VARS;
    let (counts, shifted_counts) = iop::sorted_counts(pixels);
    let counts = iop::poly(counts);

    // H goes into the transcript before the challenges, so it cannot be chosen
    // after seeing them (attack A3).
    let counts_commitment = check(Pcs::commit(pcs, &counts), "committing to range counts")?;
    append(transcript, b"hCom(x)", &counts_commitment)?;
    let offset = challenge(transcript, b"alpha")?;
    let mix = challenge(transcript, b"alpha")?;

    // f = (channel | table) + offset + mix * (channel | shifted table)
    let image = &channel.evaluations;
    let f: Vec<F> = (0..2 * half)
        .into_par_iter()
        .map(|x| {
            if x < half {
                image[x] + offset + mix * image[x]
            } else {
                table[x - half] + offset + mix * shifted_table[x - half]
            }
        })
        .collect();
    // g = counts + offset + mix * shifted counts
    let g: Vec<F> = counts
        .evaluations
        .par_iter()
        .zip(&shifted_counts)
        .map(|(count, shifted)| *count + offset + mix * shifted)
        .collect();
    drop(shifted_counts);

    let mut fraction = g.clone();
    batch_inversion(&mut fraction);
    if fraction.par_iter().any(|inverse| inverse.is_zero()) {
        return Err(anyhow!("range product check: a denominator is zero"));
    }
    fraction
        .par_iter_mut()
        .zip(&f)
        .for_each(|(inverse, numerator)| *inverse *= numerator);

    // p1 and p2 read the pairs of (fraction | product) the product is built from.
    let size = 2 * half;
    let mut product = Vec::with_capacity(size);
    for x in 0..size - 1 {
        let pair = 2 * (x % half);
        let value = if x < half {
            fraction[pair] * fraction[pair + 1]
        } else {
            product[pair] * product[pair + 1]
        };
        product.push(value);
    }
    product.push(F::zero());

    let fraction = iop::poly(fraction);
    let product = iop::poly(product);
    let fraction_commitment = check(Pcs::commit(pcs, &fraction), "committing to the fraction")?;
    let product_commitment = check(Pcs::commit(pcs, &product), "committing to the product")?;
    append(transcript, b"frac(x)", &fraction_commitment)?;
    append(transcript, b"prod(x)", &product_commitment)?;
    let alpha = challenge(transcript, b"alpha")?;

    let r = check(
        transcript.get_and_append_challenge_vectors(b"0check r", num_vars),
        "deriving the zero check point",
    )?;
    let pairs = |offset: usize| -> Vec<F> {
        (0..size)
            .into_par_iter()
            .map(|x| {
                let source = if x < half { &fraction } else { &product };
                source.evaluations[2 * (x % half) + offset]
            })
            .collect()
    };
    // (product - p1 * p2 + alpha * (g * fraction - f)) * eq
    let mut rounds = Dense::new(
        num_vars,
        vec![
            product.evaluations.clone(),
            pairs(0),
            pairs(1),
            g,
            fraction.evaluations.clone(),
            f,
            eq::table(&r),
        ],
        vec![
            (F::one(), vec![0, 6]),
            (-F::one(), vec![1, 2, 6]),
            (alpha, vec![3, 4, 6]),
            (-alpha, vec![5, 6]),
        ],
    );
    let sumcheck = sumcheck::prove(&mut rounds, transcript)?;
    Ok(RangeCheck {
        sumcheck,
        counts,
        counts_commitment,
        product,
        product_commitment,
        fraction,
        fraction_commitment,
    })
}
