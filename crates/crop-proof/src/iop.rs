//! What the prover and the verifier both rebuild from public data: the
//! challenges, the weights, the range check's tables and the opening points.
//! The IOPs are in `lean`, and in `reference` for the prover it replaced.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use ark_ff::{One, Zero};
use ark_poly::DenseMultilinearExtension;
use transcript::IOPTranscript;

use crate::{Rect, Size, Turn, F, NUM_VARS};

pub(crate) type Poly = Arc<DenseMultilinearExtension<F>>;

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

/// The multilinear extension of a vector over the original's pixels,
/// zero-padded to `NUM_VARS` variables whatever the original's size.
pub(crate) fn image_poly(mut values: Vec<F>) -> Poly {
    assert!(
        values.len() <= 1 << NUM_VARS,
        "more values than the polynomial holds"
    );
    values.resize(1 << NUM_VARS, F::zero());
    poly(values)
}

#[cfg(feature = "attack-hooks")]
pub(crate) fn field_values(pixels: &[u8]) -> Vec<F> {
    pixels.iter().map(|&pixel| F::from(pixel)).collect()
}

pub(crate) fn challenge(transcript: &mut IOPTranscript<F>, label: &'static [u8]) -> Result<F> {
    check(
        transcript.get_and_append_challenge(label),
        "deriving a challenge",
    )
}

/// 128 row weights r and `r^T A` over the original's pixels, which turn the
/// fingerprint check into one sumcheck per channel.
pub(crate) fn hash_weights(
    transcript: &mut IOPTranscript<F>,
    original: Size,
) -> Result<(Vec<F>, Vec<F>)> {
    let weights = (0..fingerprint::HASH_ROWS)
        .map(|_| challenge(transcript, b"alpha"))
        .collect::<Result<Vec<_>>>()?;
    let combined = fingerprint::combine_rows(&weights, original.pixels());
    Ok((weights, combined))
}

/// `S^T r` for the selection S of the edit: published pixel (u, v) is pixel
/// (x, y) of the rectangle before the turn, which is original pixel
/// (rect.x + x, rect.y + y).
pub(crate) fn crop_weights(original: Size, rect: Rect, turn: Turn, r: &[F]) -> Vec<F> {
    let published = turn.size(rect.size());
    let mut weights = vec![F::zero(); original.pixels()];
    for v in 0..published.height {
        for u in 0..published.width {
            let (x, y) = turn.source(rect.size(), u, v);
            weights[(rect.y + y) * original.width + rect.x + x] = r[v * published.width + u];
        }
    }
    weights
}

pub(crate) fn times_x(slot: u64, size: u64, reduction: u64) -> u64 {
    let mut next = slot << 1;
    if next & size != 0 {
        next ^= reduction;
    }
    next & (size - 1)
}

/// The lookup table T (slot g^i holds i for i in 1..=255) and its shift
/// (slot g^(i+1) holds i), over NUM_VARS variables.
pub(crate) fn range_tables() -> (Vec<F>, Vec<F>) {
    let slots = range_table_slots(NUM_VARS);
    let mut table = vec![F::zero(); 1 << NUM_VARS];
    let mut shifted = vec![F::zero(); 1 << NUM_VARS];
    for (value, pair) in (1..=MAX_PIXEL).zip(slots.windows(2)) {
        table[pair[0]] = F::from(value);
        shifted[pair[1]] = F::from(value);
    }
    (table, shifted)
}

/// The first 256 slots of the walk through GF(2^num_vars) by repeated
/// multiplication by x, from slot 1: value i of the lookup table sits in
/// slot number i - 1 of the walk, and in slot number i of its shift.
pub(crate) fn range_table_slots(num_vars: usize) -> Vec<usize> {
    let size = 1u64 << num_vars;
    let reduction = IRREDUCIBLE[num_vars] - size;
    let mut slots = Vec::with_capacity(MAX_PIXEL as usize + 1);
    let mut slot = 1;
    for _ in 0..=MAX_PIXEL {
        slots.push(slot as usize);
        slot = times_x(slot, size, reduction);
    }
    slots
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

#[cfg(feature = "attack-hooks")]
pub(crate) fn sorted_counts(pixels: &[u8]) -> (Vec<F>, Vec<F>) {
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

#[cfg(test)]
mod tests {
    use ark_ff::UniformRand;
    use rand::SeedableRng;

    use super::*;
    use crate::{edit, RgbImage};

    /// The verifier evaluates these tables. They are pinned against the loop
    /// that built them before `range_table_slots` existed, kept here word for
    /// word, and against slots worked out by hand.
    #[test]
    fn range_tables_match_the_loop_they_replaced() {
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
        let (rebuilt, rebuilt_shifted) = range_tables();
        assert!(rebuilt == table, "the lookup table changed");
        assert!(rebuilt_shifted == shifted, "the shifted table changed");

        // Slot x^(i-1) holds i. The walk leaves the powers of two after
        // x^18, because x^19 = x^5 + x^2 + x + 1.
        assert_eq!(NUM_VARS, 19);
        assert_eq!(rebuilt[1], F::from(1u32));
        assert_eq!(rebuilt[1 << 18], F::from(19u32));
        assert_eq!(rebuilt[0b10_0111], F::from(20u32));
        assert_eq!(rebuilt_shifted[1 << 1], F::from(1u32));
        assert_eq!(rebuilt_shifted[0b10_0111], F::from(19u32));
        for table in [&rebuilt, &rebuilt_shifted] {
            let filled = table.iter().filter(|value| !value.is_zero()).count();
            assert_eq!(filled, MAX_PIXEL as usize);
        }
    }

    #[test]
    fn weights_select_the_turned_rectangle() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let original = Size {
            width: 7,
            height: 5,
        };
        let channels =
            std::array::from_fn(|c| (0..original.pixels()).map(|i| (i * 3 + c) as u8).collect());
        let image = RgbImage::new(original, channels).unwrap();
        let rect = Rect {
            x: 2,
            y: 1,
            width: 4,
            height: 3,
        };
        for quarters in 0..4 {
            let turn = Turn::new(quarters).unwrap();
            let published = edit(&image, rect, turn).unwrap();
            let r: Vec<F> = (0..rect.size().pixels())
                .map(|_| F::rand(&mut rng))
                .collect();
            let weights = crop_weights(original, rect, turn, &r);
            for channel in 0..3 {
                let over_original = weights
                    .iter()
                    .zip(&image.channels()[channel])
                    .fold(F::zero(), |sum, (w, &p)| sum + *w * F::from(p));
                let over_published = r
                    .iter()
                    .zip(&published.channels()[channel])
                    .fold(F::zero(), |sum, (w, &p)| sum + *w * F::from(p));
                assert_eq!(over_original, over_published, "turn {turn}");
            }
        }
    }
}
