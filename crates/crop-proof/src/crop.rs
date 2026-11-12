use std::{
    marker::PhantomData,
    panic::{catch_unwind, AssertUnwindSafe},
};

use anyhow::{anyhow, ensure, Result};
use arithmetic::{build_eq_x_r_vec, eq_eval, VPAuxInfo};
use ark_bls12_381::Bls12_381;
use ark_ec::{pairing::Pairing, scalar_mul::variable_base::VariableBaseMSM, CurveGroup};
use ark_ff::{BigInteger, One, PrimeField, Zero};
use ark_poly::MultilinearExtension;
use sha2::{Digest, Sha256};
use subroutines::{
    pcs::prelude::{
        Commitment, MultilinearProverParam, MultilinearVerifierParam, PolynomialCommitmentScheme,
    },
    poly_iop::{prelude::ProductCheck, PolyIOP},
};
use transcript::IOPTranscript;

use crate::{
    edit,
    iop::{self, challenge, check, image_poly, poly, Opening, OpeningPoints, Poly},
    wire::{self, BatchOpening, CropProof, Sumcheck},
    Fingerprint, Pcs, ProverParams, Rect, RgbImage, Size, Turn, VerifierParams, F, NUM_VARS,
    PROTOCOL,
};

/// Proves that `published` is the `rect` of `original` turned by `turn`,
/// where the fingerprint of `original` is public.
pub fn prove(
    params: &ProverParams,
    original: &RgbImage,
    published: &RgbImage,
    rect: Rect,
    turn: Turn,
    fingerprint: &Fingerprint,
) -> Result<Vec<u8>> {
    ensure!(
        Fingerprint::of(original)? == *fingerprint,
        "the fingerprint does not match the original image"
    );
    ensure!(
        edit(original, rect, turn)? == *published,
        "the published image is not the {rect} of the original turned {turn}"
    );
    let statement = statement(
        rect,
        turn,
        fingerprint.size,
        &fingerprint.elements()?,
        published,
    );
    wire::encode(&prove_core(&params.pcs, original, rect, turn, &statement)?)
}

/// Checks a crop proof against the published pixels, the claimed rectangle
/// and turn, and the public fingerprint.
pub fn verify(
    params: &VerifierParams,
    published: &RgbImage,
    rect: Rect,
    turn: Turn,
    fingerprint: &Fingerprint,
    proof: &[u8],
) -> Result<()> {
    // The size comes from the signed fingerprint and nowhere else.
    let original = fingerprint.size;
    let hashes = fingerprint.elements()?;
    rect.check(original)?;
    ensure!(
        published.size() == turn.size(rect.size()),
        "the published image is {}, but the claimed crop is {rect} turned {turn}",
        published.size()
    );
    let proof = wire::decode(proof)?;
    // A malformed proof can reach assertions inside the arkworks and hyperplonk verifiers.
    catch_unwind(AssertUnwindSafe(|| {
        verify_core(
            &params.pcs,
            published,
            rect,
            turn,
            original,
            &hashes,
            &proof,
        )
    }))
    .map_err(|_| anyhow!("the image proof is malformed"))?
}

/// Everything the proof is about: the original's size, the rectangle, the
/// turn, the fingerprint and the published pixels. Hashed into the transcript
/// before anything else, so every challenge depends on the whole statement and
/// a proof cannot be replayed against another fingerprint, size or image.
pub(crate) fn statement(
    rect: Rect,
    turn: Turn,
    original: Size,
    hashes: &[Vec<F>; 3],
    published: &RgbImage,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(PROTOCOL.as_bytes());
    for value in [original.width, original.height] {
        hasher.update((value as u64).to_le_bytes());
    }
    hasher.update(rect.bytes());
    hasher.update(turn_bytes(turn));
    for element in hashes.iter().flatten() {
        hasher.update(element.into_bigint().to_bytes_be());
    }
    for channel in published.channels() {
        hasher.update(channel);
    }
    hasher.finalize().into()
}

/// A transcript that starts from the statement.
pub(crate) fn new_transcript(statement: &[u8; 32]) -> Result<IOPTranscript<F>> {
    let mut transcript = <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::init_transcript();
    check(
        transcript.append_message(b"statement", statement),
        "appending the statement to the transcript",
    )?;
    Ok(transcript)
}

pub(crate) fn append(
    transcript: &mut IOPTranscript<F>,
    label: &'static [u8],
    commitment: &Commitment<Bls12_381>,
) -> Result<()> {
    check(
        transcript.append_serializable_element(label, commitment),
        "appending to the transcript",
    )
}

fn prove_core(
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
    let hash = iop::prove_products(&image_poly(combined_rows), &channels, &mut transcript)?;
    let mut ranges = Vec::with_capacity(3);
    for (channel, pixels) in channels.iter().zip(original.channels()) {
        ranges.push(iop::prove_range(channel, pixels, pcs, &mut transcript)?);
    }
    let crop_challenges = crop_challenges(&mut transcript, rect, turn)?;
    let crop = iop::prove_products(
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

/// Replays the verifier up to the crop challenges. Read-only: it runs the same
/// shared prefix the verifier runs, it does not relax any check.
#[cfg(feature = "attack-hooks")]
pub(crate) fn replay_crop_challenges(
    rect: Rect,
    turn: Turn,
    original: Size,
    hashes: &[Vec<F>; 3],
    published: &RgbImage,
    proof: &CropProof,
) -> Result<Vec<F>> {
    Ok(verify_prefix(rect, turn, original, hashes, published, proof)?.crop_challenges)
}

/// Commits to one channel exactly as `prove_core` does, for the linkage test.
#[cfg(feature = "attack-hooks")]
pub(crate) fn commit_channel(
    pcs: &MultilinearProverParam<Bls12_381>,
    pixels: &[u8],
) -> Result<Commitment<Bls12_381>> {
    check(
        Pcs::commit(pcs, &image_poly(iop::field_values(pixels))),
        "committing to the image",
    )
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

fn evaluate(polynomial: &Poly, point: &[F]) -> Result<F> {
    polynomial
        .evaluate(point)
        .ok_or_else(|| anyhow!("evaluation point has {} coordinates", point.len()))
}

/// One challenge per published pixel, after binding the rectangle and the
/// turn so a proof cannot be presented for another edit.
pub(crate) fn crop_challenges(
    transcript: &mut IOPTranscript<F>,
    rect: Rect,
    turn: Turn,
) -> Result<Vec<F>> {
    check(
        transcript.append_message(b"crop", &rect.bytes()),
        "appending the crop to the transcript",
    )?;
    if turn != Turn::NONE {
        check(
            transcript.append_message(b"turn", &turn_bytes(turn)),
            "appending the turn to the transcript",
        )?;
    }
    check(
        transcript.get_and_append_challenge_vectors(b"frievald", rect.size().pixels()),
        "deriving crop challenges",
    )
}

/// Empty for no turn, so a proof made before turns existed still verifies.
/// Otherwise a tag and the quarters; the rest of the statement has a length
/// fixed by the rectangle, so the two encodings cannot collide.
fn turn_bytes(turn: Turn) -> Vec<u8> {
    match turn.quarters() {
        0 => Vec::new(),
        quarters => [b"turn".as_slice(), &[quarters]].concat(),
    }
}

fn aux_info(num_variables: usize) -> VPAuxInfo<F> {
    VPAuxInfo {
        max_degree: 2,
        num_variables,
        phantom: PhantomData,
    }
}

struct RangeClaim {
    offset: F,
    mix: F,
    alpha: F,
    expected: F,
}

/// What the verifier derives up to the crop challenges. Shared with
/// `hooks::crop_challenges` so the challenges have one source.
struct VerifyPrefix {
    transcript: IOPTranscript<F>,
    combined_rows: Vec<F>,
    hash_claims: Vec<SumcheckClaim>,
    range_claims: Vec<RangeClaim>,
    crop_challenges: Vec<F>,
}

fn verify_prefix(
    rect: Rect,
    turn: Turn,
    original: Size,
    hashes: &[Vec<F>; 3],
    published: &RgbImage,
    proof: &CropProof,
) -> Result<VerifyPrefix> {
    let commitments = &proof.commitments;
    let mut transcript = new_transcript(&statement(rect, turn, original, hashes, published))?;
    for commitment in &commitments[..3] {
        append(&mut transcript, b"img(x)", commitment)?;
    }

    let (row_weights, combined_rows) = iop::hash_weights(&mut transcript, original)?;
    let mut hash_claims = Vec::with_capacity(3);
    for (sumcheck, hash) in proof.hash.iter().zip(hashes) {
        let claimed = row_weights
            .iter()
            .zip(hash)
            .fold(F::zero(), |sum, (weight, value)| sum + *weight * value);
        hash_claims.push(verify_sumcheck(
            claimed,
            sumcheck,
            &aux_info(NUM_VARS),
            &mut transcript,
        )?);
    }

    let mut range_claims = Vec::with_capacity(3);
    for channel in 0..3 {
        append(&mut transcript, b"hCom(x)", &commitments[3 + 3 * channel])?;
        let offset = challenge(&mut transcript, b"alpha")?;
        let mix = challenge(&mut transcript, b"alpha")?;
        let (alpha, expected) = verify_product_check(
            &proof.range[channel],
            &commitments[4 + 3 * channel],
            &commitments[5 + 3 * channel],
            &aux_info(NUM_VARS + 1),
            &mut transcript,
        )
        .map_err(|error| anyhow!("range check for channel {channel} rejected: {error}"))?;
        range_claims.push(RangeClaim {
            offset,
            mix,
            alpha,
            expected,
        });
    }

    // The statement opened the transcript, so these challenges depend on the
    // published pixels and cannot be fixed before choosing them.
    let crop_challenges = crop_challenges(&mut transcript, rect, turn)?;
    Ok(VerifyPrefix {
        transcript,
        combined_rows,
        hash_claims,
        range_claims,
        crop_challenges,
    })
}

fn verify_core(
    pcs: &MultilinearVerifierParam<Bls12_381>,
    published: &RgbImage,
    rect: Rect,
    turn: Turn,
    original: Size,
    hashes: &[Vec<F>; 3],
    proof: &CropProof,
) -> Result<()> {
    let commitments = &proof.commitments;
    let VerifyPrefix {
        mut transcript,
        combined_rows,
        hash_claims,
        range_claims,
        crop_challenges,
    } = verify_prefix(rect, turn, original, hashes, published, proof)?;
    let mut crop_claims = Vec::with_capacity(3);
    for (sumcheck, pixels) in proof.crop.iter().zip(published.channels()) {
        let claimed = crop_challenges
            .iter()
            .zip(pixels)
            .fold(F::zero(), |sum, (weight, &pixel)| {
                sum + *weight * F::from(pixel)
            });
        crop_claims.push(verify_sumcheck(
            claimed,
            sumcheck,
            &aux_info(NUM_VARS),
            &mut transcript,
        )?);
    }

    let OpeningPoints {
        points,
        range_starts,
    } = iop::opening_points(
        proof
            .hash
            .each_ref()
            .map(|sumcheck| sumcheck.point.as_slice()),
        proof
            .range
            .each_ref()
            .map(|sumcheck| sumcheck.point.as_slice()),
        proof
            .crop
            .each_ref()
            .map(|sumcheck| sumcheck.point.as_slice()),
    );
    let image = check_openings(
        pcs,
        commitments,
        &points,
        &iop::image_openings(),
        &proof.image_openings,
        &mut transcript,
    )?;
    let range = check_openings(
        pcs,
        commitments,
        &points,
        &iop::range_openings(),
        &proof.range_openings,
        &mut transcript,
    )
    .map_err(|error| anyhow!("range check openings rejected: {error}"))?;

    let (table, shifted_table) = iop::range_tables();
    let (table, shifted_table) = (poly(table), poly(shifted_table));
    for (channel, claim) in range_claims.iter().enumerate() {
        let r = &proof.range[channel].point;
        let (without_last, last) = (&r[..r.len() - 1], r[r.len() - 1]);
        let at = |k: usize| range[11 * channel + k];
        let pixel = image[1 + 3 * channel];
        let start = range_starts[channel];

        let left_0 = last * at(7) + (F::one() - last) * at(8);
        let left_1 = last * at(9) + (F::one() - last) * at(10);
        let product_step = at(5) - left_0 * left_1;

        let merged = |table_value: F| (F::one() - last) * pixel + last * table_value;
        let numerator = claim.offset
            + merged(evaluate(&table, without_last)?)
            + claim.mix * merged(evaluate(&shifted_table, without_last)?);
        let denominator =
            claim.offset + at(1) + claim.mix * (start * at(2) + (F::one() - start) * at(3));
        let fraction_step = (denominator * at(6) - numerator) * claim.alpha;

        ensure!(
            product_step + fraction_step == claim.expected,
            "range check for channel {channel} rejected"
        );
    }

    let combined_rows = image_poly(combined_rows);
    for (channel, claim) in hash_claims.iter().enumerate() {
        ensure!(
            claim.expected == evaluate(&combined_rows, &claim.point)? * image[3 * channel],
            "fingerprint check for channel {channel} rejected"
        );
    }
    let crop_weights = image_poly(iop::crop_weights(original, rect, turn, &crop_challenges));
    for (channel, claim) in crop_claims.iter().enumerate() {
        ensure!(
            claim.expected == evaluate(&crop_weights, &claim.point)? * image[2 + 3 * channel],
            "crop check for channel {channel} rejected"
        );
    }
    Ok(())
}

fn check_openings<'a>(
    pcs: &MultilinearVerifierParam<Bls12_381>,
    commitments: &[Commitment<Bls12_381>],
    points: &[Vec<F>],
    openings: &[Opening],
    proof: &'a BatchOpening,
    transcript: &mut IOPTranscript<F>,
) -> Result<&'a [F]> {
    let selected: Vec<_> = openings
        .iter()
        .map(|opening| commitments[opening.commitment])
        .collect();
    let at: Vec<_> = openings
        .iter()
        .map(|opening| points[opening.point].clone())
        .collect();
    ensure!(
        verify_batch_opening(pcs, &selected, &at, proof, transcript)?,
        "KZG batch opening rejected"
    );
    for (index, opening) in openings.iter().enumerate() {
        if let Some(value) = opening.fixed {
            ensure!(
                proof.evaluations[index] == value,
                "opening {index} does not have its fixed value"
            );
        }
    }
    Ok(&proof.evaluations)
}

struct SumcheckClaim {
    point: Vec<F>,
    expected: F,
}

/// Evaluates the degree-d round polynomial given by its values at 0..=d.
fn interpolate(values: &[F], at: F) -> F {
    let mut result = F::zero();
    for (i, value) in values.iter().enumerate() {
        let mut basis = F::one();
        for j in (0..values.len()).filter(|&j| j != i) {
            basis *= (at - F::from(j as u64)) / (F::from(i as u64) - F::from(j as u64));
        }
        result += *value * basis;
    }
    result
}

fn verify_sumcheck(
    claimed_sum: F,
    proof: &Sumcheck,
    aux_info: &VPAuxInfo<F>,
    transcript: &mut IOPTranscript<F>,
) -> Result<SumcheckClaim> {
    ensure!(
        proof.rounds.len() == aux_info.num_variables,
        "sumcheck has the wrong number of rounds"
    );
    check(
        transcript.append_serializable_element(b"aux info", aux_info),
        "appending to the transcript",
    )?;
    let mut expected = claimed_sum;
    let mut point = Vec::with_capacity(aux_info.num_variables);
    for round in &proof.rounds {
        ensure!(
            round.len() == aux_info.max_degree + 1,
            "sumcheck round has {} evaluations",
            round.len()
        );
        ensure!(
            round[0] + round[1] == expected,
            "sumcheck round does not match its claim"
        );
        check(
            transcript.append_serializable_element(b"prover msg", round),
            "appending to the transcript",
        )?;
        let r = challenge(transcript, b"Internal round")?;
        expected = interpolate(round, r);
        point.push(r);
    }
    ensure!(
        proof.point == point,
        "sumcheck point does not match the transcript"
    );
    Ok(SumcheckClaim { point, expected })
}

fn verify_zero_check(
    proof: &Sumcheck,
    aux_info: &VPAuxInfo<F>,
    transcript: &mut IOPTranscript<F>,
) -> Result<SumcheckClaim> {
    let first = proof
        .rounds
        .first()
        .ok_or_else(|| anyhow!("zero check is empty"))?;
    ensure!(
        first.len() >= 2 && first[0] + first[1] == F::zero(),
        "zero check does not sum to zero"
    );
    let r = check(
        transcript.get_and_append_challenge_vectors(b"0check r", aux_info.num_variables),
        "deriving zero-check challenges",
    )?;
    let mut degree_raised = aux_info.clone();
    degree_raised.max_degree += 1;
    let claim = verify_sumcheck(F::zero(), proof, &degree_raised, transcript)?;
    let eq = check(eq_eval(&claim.point, &r), "evaluating eq")?;
    ensure!(!eq.is_zero(), "zero-check challenge evaluated eq to zero");
    Ok(SumcheckClaim {
        point: claim.point,
        expected: claim.expected / eq,
    })
}

/// Returns the product check's challenge and the value its zero check expects.
fn verify_product_check(
    zero_check: &Sumcheck,
    product: &Commitment<Bls12_381>,
    fraction: &Commitment<Bls12_381>,
    aux_info: &VPAuxInfo<F>,
    transcript: &mut IOPTranscript<F>,
) -> Result<(F, F)> {
    append(transcript, b"frac(x)", fraction)?;
    append(transcript, b"prod(x)", product)?;
    let alpha = challenge(transcript, b"alpha")?;
    let claim = verify_zero_check(zero_check, aux_info, transcript)?;
    Ok((alpha, claim.expected))
}

fn verify_batch_opening(
    pcs: &MultilinearVerifierParam<Bls12_381>,
    commitments: &[Commitment<Bls12_381>],
    points: &[Vec<F>],
    proof: &BatchOpening,
    transcript: &mut IOPTranscript<F>,
) -> Result<bool> {
    ensure!(
        !commitments.is_empty()
            && commitments.len() == points.len()
            && proof.evaluations.len() == points.len(),
        "batch opening has mismatched commitments, points and evaluations"
    );
    for point in points {
        check(
            transcript.append_serializable_element(b"eval_point", point),
            "appending to the transcript",
        )?;
    }
    for evaluation in &proof.evaluations {
        check(
            transcript.append_field_element(b"eval", evaluation),
            "appending to the transcript",
        )?;
    }
    let log_count = commitments.len().next_power_of_two().trailing_zeros() as usize;
    let t = check(
        transcript.get_and_append_challenge_vectors(b"t", log_count),
        "deriving batch challenges",
    )?;
    let eq_t = check(build_eq_x_r_vec(&t), "building eq")?;

    let a2 = &proof.sumcheck.point;
    let mut scalars = Vec::with_capacity(commitments.len());
    let mut sum = F::zero();
    for (index, point) in points.iter().enumerate() {
        scalars.push(check(eq_eval(a2, point), "evaluating eq")? * eq_t[index]);
        sum += eq_t[index] * proof.evaluations[index];
    }
    let bases: Vec<_> = commitments.iter().map(|commitment| commitment.0).collect();
    let combined = <Bls12_381 as Pairing>::G1::msm_unchecked(&bases, &scalars);

    let claim = verify_sumcheck(sum, &proof.sumcheck, &aux_info(a2.len()), transcript)?;
    check(
        Pcs::verify(
            pcs,
            &Commitment(combined.into_affine()),
            &claim.point,
            &claim.expected,
            &proof.kzg,
        ),
        "verifying the KZG opening",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ark_poly::DenseMultilinearExtension;
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn no_turn_leaves_the_statement_as_before_turns() {
        assert!(turn_bytes(Turn::NONE).is_empty());
        let size = Size {
            width: 2,
            height: 2,
        };
        let rect = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        let image = RgbImage::new(size, std::array::from_fn(|_| vec![0; 4])).unwrap();
        let hashes: [Vec<F>; 3] = std::array::from_fn(|_| vec![F::one()]);
        let statements: Vec<_> = (0..4)
            .map(|q| statement(rect, Turn::new(q).unwrap(), size, &hashes, &image))
            .collect();
        for (i, a) in statements.iter().enumerate() {
            for b in &statements[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn sumcheck_with_a_false_claim_is_rejected() {
        let proof = Sumcheck {
            point: vec![F::zero()],
            rounds: vec![vec![F::one(); 3]],
        };
        let mut transcript = new_transcript(&[0; 32]).unwrap();
        assert!(verify_sumcheck(F::zero(), &proof, &aux_info(1), &mut transcript).is_err());
    }

    #[test]
    fn product_check_matches_the_hyperplonk_verifier() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let srs = Pcs::gen_srs_for_testing(&mut rng, 2).unwrap();
        let (prover, _) = Pcs::trim(&srs, None, Some(2)).unwrap();
        let values = [1u64, 2, 3, 4].map(F::from).to_vec();
        let polynomial = Arc::new(DenseMultilinearExtension::from_evaluations_vec(2, values));
        let mut prover_transcript = new_transcript(&[0; 32]).unwrap();
        let (proof, _, _) = <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::prove(
            &prover,
            std::slice::from_ref(&polynomial),
            std::slice::from_ref(&polynomial),
            &mut prover_transcript,
        )
        .unwrap();

        let mut upstream_transcript = new_transcript(&[0; 32]).unwrap();
        let upstream = <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::verify(
            &proof,
            &aux_info(2),
            &mut upstream_transcript,
        )
        .unwrap();
        let mut ours_transcript = new_transcript(&[0; 32]).unwrap();
        let (alpha, expected) = verify_product_check(
            &proof.zero_check_proof.clone().into(),
            &proof.prod_x_comm,
            &proof.frac_comm,
            &aux_info(2),
            &mut ours_transcript,
        )
        .unwrap();

        assert_eq!(alpha, upstream.alpha);
        assert_eq!(expected, upstream.zero_check_sub_claim.expected_evaluation);
        assert_eq!(
            ours_transcript.get_and_append_challenge(b"next").unwrap(),
            upstream_transcript
                .get_and_append_challenge(b"next")
                .unwrap()
        );
    }
}
