//! The prover as first written, on hyperplonk's own sumcheck, product check
//! and batch opening. It needs about 6 GB of memory. The tests keep it to
//! check that the prover in `lean` writes the same proof, and the forgers of
//! the security analysis are built from its parts.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use arithmetic::{merge_polynomials, VirtualPolynomial};
use ark_bls12_381::Bls12_381;
use ark_ff::One;
use ark_poly::DenseMultilinearExtension;
use subroutines::{
    pcs::prelude::{Commitment, MultilinearProverParam, PolynomialCommitmentScheme},
    poly_iop::{
        prelude::{ProductCheck, SumCheck},
        PolyIOP,
    },
};
use transcript::IOPTranscript;

use crate::{
    crop::{append, crop_challenges, evaluate, new_transcript},
    iop::{
        self, challenge, check, image_poly, poly, range_tables, sorted_counts, Opening,
        OpeningPoints, Poly,
    },
    wire::{BatchOpening, CropProof, Sumcheck},
    Pcs, Rect, RgbImage, Turn, F, NUM_VARS,
};

pub(crate) type SumCheckProof = <PolyIOP<F> as SumCheck<F>>::SumCheckProof;
pub(crate) type ProductCheckProof = <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::ProductCheckProof;

pub(crate) fn prove_core(
    pcs: &MultilinearProverParam<Bls12_381>,
    original: &RgbImage,
    rect: Rect,
    turn: Turn,
    statement: &[u8; 32],
) -> Result<CropProof> {
    let size = original.size();
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

    let (_, combined_rows) = iop::hash_weights(&mut transcript, size)?;
    let hash = prove_products(&image_poly(combined_rows), &channels, &mut transcript)?;
    let mut ranges = Vec::with_capacity(3);
    for (channel, pixels) in channels.iter().zip(original.channels()) {
        ranges.push(prove_range(channel, pixels, pcs, &mut transcript)?);
    }
    let crop_challenges = crop_challenges(&mut transcript, rect, turn)?;
    let crop = prove_products(
        &image_poly(iop::crop_weights(size, rect, turn, &crop_challenges)),
        &channels,
        &mut transcript,
    )?;

    for range in &ranges {
        commitments.extend([
            range.counts_commitment,
            range.proof.prod_x_comm,
            range.proof.frac_comm,
        ]);
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

pub(crate) fn open(
    pcs: &MultilinearProverParam<Bls12_381>,
    polynomials: &[&Poly],
    points: &[Vec<F>],
    openings: &[Opening],
    transcript: &mut IOPTranscript<F>,
) -> Result<BatchOpening> {
    let mut selected = Vec::with_capacity(openings.len());
    let mut at = Vec::with_capacity(openings.len());
    let mut values = Vec::with_capacity(openings.len());
    for opening in openings {
        let polynomial = polynomials[opening.commitment];
        let point = &points[opening.point];
        let value = match opening.fixed {
            Some(value) => value,
            None => evaluate(polynomial, point)?,
        };
        selected.push(polynomial.clone());
        at.push(point.clone());
        values.push(value);
    }
    Ok(check(
        Pcs::multi_open(pcs, &selected, &at, &values, transcript),
        "opening commitments",
    )?
    .into())
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
    pub counts_commitment: Commitment<Bls12_381>,
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

    // H goes into the transcript before the challenges, so it cannot be chosen
    // after seeing them (attack A3).
    let counts_commitment = check(Pcs::commit(params, &right[0]), "committing to range counts")?;
    check(
        transcript.append_serializable_element(b"hCom(x)", &counts_commitment),
        "appending to the transcript",
    )?;
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
        counts_commitment,
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
