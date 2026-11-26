//! The crop prover. Same steps and transcript as the reference prover in
//! `reference.rs`, which the tests keep to compare proofs byte for byte.

use anyhow::Result;
use ark_ec::CurveGroup;
use subroutines::pcs::prelude::Commitment;

use super::{msm, open, products, range, srs::Srs, Values};
use crate::{
    crop::{append, crop_challenges, new_transcript},
    iop::{self, OpeningPoints},
    probe::mark,
    wire::CropProof,
    Rect, RgbImage, Turn, NUM_VARS,
};

pub(crate) fn prove_core(
    srs: &Srs,
    original: &RgbImage,
    rect: Rect,
    turn: Turn,
    statement: &[u8; 32],
) -> Result<CropProof> {
    let size = original.size();
    let pixels = original.channels().each_ref().map(Vec::as_slice);
    mark("commit image");
    let basis = srs.basis(NUM_VARS);
    let mut commitments: Vec<_> = pixels
        .iter()
        .map(|channel| Commitment(msm::msm_bytes(&basis, channel).into_affine()))
        .collect();
    let mut transcript = new_transcript(statement)?;
    for commitment in &commitments {
        append(&mut transcript, b"img(x)", commitment)?;
    }

    mark("hash weights");
    let (_, combined_rows) = iop::hash_weights(&mut transcript, size)?;
    mark("hash sumcheck");
    let hash = products::prove_products(NUM_VARS, &combined_rows, pixels, &mut transcript)?;
    drop(combined_rows);

    let mut ranges = Vec::with_capacity(3);
    const RANGE: [&str; 3] = ["range check 0", "range check 1", "range check 2"];
    for (channel, phase) in pixels.iter().zip(RANGE) {
        mark(phase);
        ranges.push(range::prove_range(srs, NUM_VARS, channel, &mut transcript)?);
    }

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
            [
                Values::Bytes(&range.counts),
                Values::Field(&range.product),
                Values::Field(&range.fraction),
            ]
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
