//! The fingerprint and crop sumchecks: `sum of weights * channel`, where the
//! channel is the image's bytes. The first rounds read the bytes and the
//! weights as they are, so the only tables are the two folded ones, a
//! fraction of the size of the inputs.

use anyhow::{ensure, Result};
use ark_ff::{Field, One, Zero};
use rayon::prelude::*;
use transcript::IOPTranscript;

use super::{
    eq,
    sumcheck::{self, Rounds},
};
use crate::{wire::Sumcheck, F, NUM_VARS};

/// Rounds answered from the inputs before the folded tables exist. Each one
/// halves the tables and costs twice the additions of the one before. Measured
/// at 19 variables on one thread, for the three sumchecks: one lazy round
/// takes 100 ms and 16 MiB, two take 98 ms and 8 MiB, three take 134 ms and
/// 4 MiB.
const LAZY_ROUNDS: usize = 2;

/// Inputs one task of a lazy round reads. Every worker fills one set of
/// buckets, 72 KiB in the last lazy round, whatever the size of the inputs.
const LAZY_CHUNK: usize = 1 << 14;

/// Table entries one task folds. Two chunks of field elements fit in a core's
/// cache, so the fold and the sums of a round read memory once.
const FOLD_CHUNK: usize = 1 << 12;

const _: () = assert!(LAZY_ROUNDS >= 1 && LAZY_CHUNK.is_multiple_of(1 << LAZY_ROUNDS));
const _: () = assert!(FOLD_CHUNK.is_power_of_two() && FOLD_CHUNK >= 4);

const BYTE_VALUES: usize = 256;

/// One sumcheck per channel of `sum over x of weights(x) * channel(x)`, in
/// channel order on the same transcript. A slice shorter than `2^num_vars`
/// stands for its values followed by zeros.
pub(crate) fn prove_products(
    num_vars: usize,
    weights: &[F],
    channels: [&[u8]; 3],
    transcript: &mut IOPTranscript<F>,
) -> Result<[Sumcheck; 3]> {
    // The tables below are sized by `num_vars`, not by the slices, which may
    // be short. So the bound has to hold before anything is reserved.
    ensure!(
        num_vars <= NUM_VARS,
        "a sumcheck over {num_vars} variables, but the proof has {NUM_VARS}"
    );
    let size = 1usize << num_vars;
    ensure!(
        weights.len() <= size,
        "{} weights for {size} values",
        weights.len()
    );
    for channel in channels {
        ensure!(
            channel.len() <= size,
            "{} channel values for {size} values",
            channel.len()
        );
    }

    // The three sumchecks fold into the same two tables.
    let lazy_rounds = LAZY_ROUNDS.min(num_vars);
    let table_len = if lazy_rounds < num_vars {
        size >> lazy_rounds
    } else {
        0
    };
    let mut weights_table = Vec::with_capacity(table_len);
    let mut channel_table = Vec::with_capacity(table_len);
    let mut prove = |channel| {
        let mut rounds = Products {
            num_vars,
            lazy_rounds,
            weights,
            channel,
            round: 0,
            challenges: Vec::with_capacity(lazy_rounds),
            weights_table: &mut weights_table,
            channel_table: &mut channel_table,
        };
        sumcheck::prove(&mut rounds, transcript)
    };
    Ok([
        prove(channels[0])?,
        prove(channels[1])?,
        prove(channels[2])?,
    ])
}

/// The prover of one sumcheck of `weights * channel`.
struct Products<'a> {
    num_vars: usize,
    lazy_rounds: usize,
    weights: &'a [F],
    channel: &'a [u8],
    /// Rounds answered so far.
    round: usize,
    /// The challenges of the lazy rounds.
    challenges: Vec<F>,
    /// The weights and the channel with the variables of every challenge so
    /// far fixed. Empty during the lazy rounds.
    weights_table: &'a mut Vec<F>,
    channel_table: &'a mut Vec<F>,
}

impl Rounds for Products<'_> {
    fn num_vars(&self) -> usize {
        self.num_vars
    }

    fn max_degree(&self) -> usize {
        2
    }

    fn round(&mut self, challenge: Option<F>) -> Vec<F> {
        let round = self.round;
        self.round += 1;
        match challenge {
            Some(r) if round > self.lazy_rounds => self.fold(r),
            _ => {
                self.challenges.extend(challenge);
                if round < self.lazy_rounds {
                    self.lazy_round()
                } else {
                    self.materialize()
                }
            }
        }
        .to_vec()
    }
}

impl Products<'_> {
    /// A round from the inputs alone, with additions and a few hundred
    /// multiplications.
    ///
    /// After k challenges r, the folded weights are
    /// `W(b, top) = sum over low of eq(r, low) * weights[b, top, low]`, where
    /// `low` is the k lowest bits of the index and `top` the next one, and
    /// the same holds for the channel. The round's polynomial needs the sums
    /// over b of `W(b, 0) C(b, 0)`, of `W(b, 1) C(b, 1)` and of the two mixed
    /// products. Each is a sum of `eq(r, i) eq(r, j) * weight * byte` over
    /// pairs of positions in the same block of `2^(k+1)` inputs. So for every
    /// pair the weight goes into a bucket chosen by the pair's coefficient
    /// and by the byte, and the multiplications come after, once per bucket.
    fn lazy_round(&self) -> [F; 3] {
        let fixed = self.challenges.len();
        let half = 1usize << fixed;
        let block = 2 * half;
        // eq(r, i) eq(r, j) depends on how many of i and j set each bit:
        // one base-3 digit per challenge.
        let groups = 3usize.pow(fixed as u32);
        let slots: Vec<usize> = (0..block * block)
            .map(|pair| {
                let (i, j) = (pair / block, pair % block);
                let class = match (i / half, j / half) {
                    (0, 0) => 0,
                    (1, 1) => 1,
                    _ => 2,
                };
                class * groups + ternary(i % half) + ternary(j % half)
            })
            .collect();

        let live = self.weights.len().min(self.channel.len());
        let sums = (0..live.div_ceil(LAZY_CHUNK))
            .into_par_iter()
            .fold(
                || vec![F::zero(); 3 * groups * BYTE_VALUES],
                |mut buckets, chunk| {
                    let end = live.min((chunk + 1) * LAZY_CHUNK);
                    for start in (chunk * LAZY_CHUNK..end).step_by(block) {
                        let weights = window(self.weights, start, block);
                        let bytes = window(self.channel, start, block);
                        for (weight, slots) in weights.iter().zip(slots.chunks_exact(block)) {
                            // The crop weights are zero outside the rectangle.
                            if weight.is_zero() {
                                continue;
                            }
                            for (&slot, &byte) in slots.iter().zip(bytes) {
                                buckets[slot * BYTE_VALUES + usize::from(byte)] += weight;
                            }
                        }
                    }
                    buckets
                },
            )
            .map(|buckets| {
                buckets
                    .chunks_exact(BYTE_VALUES)
                    .map(times_index)
                    .collect::<Vec<F>>()
            })
            .reduce(
                || vec![F::zero(); 3 * groups],
                |mut left, right| {
                    for (sum, value) in left.iter_mut().zip(right) {
                        *sum += value;
                    }
                    left
                },
            );

        let mut coefficients = vec![F::one()];
        for &r in &self.challenges {
            let keep = F::one() - r;
            coefficients = [keep * keep, keep * r, r * r]
                .iter()
                .flat_map(|digit| coefficients.iter().map(move |rest| *rest * digit))
                .collect();
        }
        let [at_zero, at_one, mixed]: [F; 3] = std::array::from_fn(|class| {
            sums[class * groups..]
                .iter()
                .zip(&coefficients)
                .map(|(sum, coefficient)| *sum * coefficient)
                .sum()
        });
        // (2 W1 - W0) (2 C1 - C0)
        [
            at_zero,
            at_one,
            at_zero + at_one.double().double() - mixed.double(),
        ]
    }

    /// Builds the two tables after the last lazy round, and answers the round
    /// from them.
    fn materialize(&mut self) -> [F; 3] {
        let fixed = self.challenges.len();
        let block = 1usize << fixed;
        let len = 1usize << (self.num_vars - fixed);
        for table in [&mut *self.weights_table, &mut *self.channel_table] {
            table.clear();
            table.resize(len, F::zero());
        }
        // A folded channel value is a sum of eq(r, j) * byte, one byte per
        // position j of the block: every such product, built by additions.
        let multiples: Vec<F> = eq::table(&self.challenges)
            .iter()
            .flat_map(|&coefficient| {
                std::iter::successors(Some(F::zero()), move |&multiple| {
                    Some(multiple + coefficient)
                })
                .take(BYTE_VALUES)
            })
            .collect();

        let (weights, channel, challenges) = (self.weights, self.channel, &self.challenges);
        let chunk = FOLD_CHUNK.min(len);
        self.weights_table
            .par_chunks_mut(chunk)
            .zip(self.channel_table.par_chunks_mut(chunk))
            .enumerate()
            .map(|(index, (weights_table, channel_table))| {
                let first = index * chunk;
                for (offset, (weight, value)) in weights_table
                    .iter_mut()
                    .zip(&mut *channel_table)
                    .enumerate()
                {
                    let start = (first + offset) << fixed;
                    *weight = fold_block(window(weights, start, block), challenges);
                    *value = window(channel, start, block)
                        .iter()
                        .zip(multiples.chunks_exact(BYTE_VALUES))
                        .map(|(&byte, multiples)| multiples[usize::from(byte)])
                        .sum();
                }
                round_sums(weights_table, channel_table)
            })
            .reduce(|| [F::zero(); 3], add)
    }

    /// Fixes the lowest variable of both tables to `r`, in place, and answers
    /// the round from the result.
    fn fold(&mut self, r: F) -> [F; 3] {
        let chunk = FOLD_CHUNK.min(self.weights_table.len());
        // Every task folds its chunk into the chunk's first half, so tasks
        // do not touch each other's entries.
        let sums = self
            .weights_table
            .par_chunks_mut(chunk)
            .zip(self.channel_table.par_chunks_mut(chunk))
            .map(|(weights, channel)| {
                fold_in_place(weights, r);
                fold_in_place(channel, r);
                round_sums(&weights[..chunk / 2], &channel[..chunk / 2])
            })
            .reduce(|| [F::zero(); 3], add);
        keep_first_halves(self.weights_table, chunk);
        keep_first_halves(self.channel_table, chunk);
        sums
    }
}

/// The values of `start..start + size`, without those past the end.
fn window<T>(values: &[T], start: usize, size: usize) -> &[T] {
    let end = values.len().min(start + size);
    values.get(start..end).unwrap_or(&[])
}

/// The number whose base-3 digits are the bits of `bits`.
fn ternary(bits: usize) -> usize {
    (0..usize::BITS)
        .filter(|bit| bits >> bit & 1 == 1)
        .map(|bit| 3usize.pow(bit))
        .sum()
}

/// `sum over v of v * values[v]` by additions alone. The running sum from the
/// top holds `values[v]` during v of the steps, so adding it at every step
/// counts `values[v]` v times.
fn times_index(values: &[F]) -> F {
    let mut from_top = F::zero();
    let mut total = F::zero();
    for value in values.iter().skip(1).rev() {
        from_top += value;
        total += from_top;
    }
    total
}

/// One entry of the folded weights: `values`, a block of at most
/// `2^challenges.len()` weights, with all its variables fixed.
fn fold_block(values: &[F], challenges: &[F]) -> F {
    let mut block = [F::zero(); 1 << LAZY_ROUNDS];
    block[..values.len()].copy_from_slice(values);
    let mut len = 1 << challenges.len();
    for &r in challenges {
        len /= 2;
        fold_in_place(&mut block[..2 * len], r);
    }
    block[0]
}

/// Writes `table` with its lowest variable fixed to `r` into its first half.
fn fold_in_place(table: &mut [F], r: F) {
    for b in 0..table.len() / 2 {
        table[b] = table[2 * b] + (table[2 * b + 1] - table[2 * b]) * r;
    }
}

/// Moves the first half of every chunk of `table` to the front and drops the
/// rest.
fn keep_first_halves(table: &mut Vec<F>, chunk: usize) {
    let half = chunk / 2;
    for index in 1..table.len() / chunk {
        table.copy_within(index * chunk..index * chunk + half, index * half);
    }
    table.truncate(table.len() / 2);
}

/// The round's polynomial at 0, 1 and 2 from two tables of even length.
fn round_sums(weights: &[F], channel: &[F]) -> [F; 3] {
    let mut sums = [F::zero(); 3];
    for (weights, channel) in weights.chunks_exact(2).zip(channel.chunks_exact(2)) {
        sums[0] += weights[0] * channel[0];
        sums[1] += weights[1] * channel[1];
        sums[2] += (weights[1].double() - weights[0]) * (channel[1].double() - channel[0]);
    }
    sums
}

fn add(mut left: [F; 3], right: [F; 3]) -> [F; 3] {
    for (sum, value) in left.iter_mut().zip(right) {
        *sum += value;
    }
    left
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ark_ff::UniformRand;
    use ark_poly::DenseMultilinearExtension;
    use rand::{rngs::StdRng, Rng, SeedableRng};

    use super::*;
    use crate::{iop::Poly, reference};

    fn padded(num_vars: usize, mut values: Vec<F>) -> Poly {
        values.resize(1 << num_vars, F::zero());
        Arc::new(DenseMultilinearExtension::from_evaluations_vec(
            num_vars, values,
        ))
    }

    /// Runs hyperplonk's sumcheck and this one on the same inputs and
    /// compares the proofs and the transcripts they leave behind.
    fn assert_matches_hyperplonk(num_vars: usize, weights: &[F], channels: [&[u8]; 3]) {
        let mut expected_transcript = IOPTranscript::<F>::new(b"products");
        let expected = reference::prove_products(
            &padded(num_vars, weights.to_vec()),
            &channels.map(|channel| padded(num_vars, crate::iop::field_values(channel))),
            &mut expected_transcript,
        )
        .unwrap()
        .map(Sumcheck::from);

        let mut transcript = IOPTranscript::<F>::new(b"products");
        let proofs = prove_products(num_vars, weights, channels, &mut transcript).unwrap();
        assert_eq!(proofs, expected, "{num_vars} variables");
        assert_eq!(
            transcript.get_and_append_challenge(b"after").unwrap(),
            expected_transcript
                .get_and_append_challenge(b"after")
                .unwrap(),
            "{num_vars} variables"
        );
    }

    fn random_weights(rng: &mut StdRng, len: usize) -> Vec<F> {
        (0..len).map(|_| F::rand(rng)).collect()
    }

    fn random_bytes(rng: &mut StdRng, len: usize) -> Vec<u8> {
        (0..len).map(|_| rng.gen()).collect()
    }

    #[test]
    fn full_inputs_match_hyperplonk() {
        let mut rng = StdRng::seed_from_u64(11);
        for num_vars in 1..=10 {
            let size = 1 << num_vars;
            let weights = random_weights(&mut rng, size);
            let channels: [Vec<u8>; 3] = std::array::from_fn(|_| random_bytes(&mut rng, size));
            assert_matches_hyperplonk(num_vars, &weights, channels.each_ref().map(Vec::as_slice));
        }
    }

    #[test]
    fn short_inputs_match_hyperplonk() {
        let mut rng = StdRng::seed_from_u64(12);
        for num_vars in 1..=10 {
            let size = 1usize << num_vars;
            for _ in 0..4 {
                let lengths: [usize; 4] = std::array::from_fn(|_| rng.gen_range(0..=size));
                let weights = random_weights(&mut rng, lengths[3]);
                let channels: [Vec<u8>; 3] =
                    std::array::from_fn(|i| random_bytes(&mut rng, lengths[i]));
                assert_matches_hyperplonk(
                    num_vars,
                    &weights,
                    channels.each_ref().map(Vec::as_slice),
                );
            }
        }
    }

    #[test]
    fn zero_inputs_match_hyperplonk() {
        let mut rng = StdRng::seed_from_u64(13);
        for num_vars in 1..=10 {
            let size = 1 << num_vars;
            let weights = random_weights(&mut rng, size);
            let zeros = vec![0; size];
            let bytes = random_bytes(&mut rng, size);
            // An all-zero channel between two others, then no values at all.
            assert_matches_hyperplonk(num_vars, &weights, [&bytes, &zeros, &bytes]);
            assert_matches_hyperplonk(num_vars, &weights, [&[], &[], &[]]);
            assert_matches_hyperplonk(num_vars, &[], [&bytes, &zeros, &bytes]);
            // Weights that are zero in places, like the crop weights.
            let sparse: Vec<F> = weights
                .iter()
                .map(|weight| if rng.gen() { *weight } else { F::zero() })
                .collect();
            assert_matches_hyperplonk(num_vars, &sparse, [&bytes, &zeros, &bytes]);
        }
    }

    /// The sizes where the work of a round starts to split: folded tables of
    /// half a task's chunk, of one chunk and of two, up to enough values for
    /// every round to run several tasks.
    #[test]
    fn inputs_of_one_chunk_and_of_many_match_hyperplonk() {
        let mut rng = StdRng::seed_from_u64(14);
        let one_chunk = (FOLD_CHUNK << LAZY_ROUNDS).ilog2() as usize;
        let many_chunks = (4 * LAZY_CHUNK.max(FOLD_CHUNK << LAZY_ROUNDS)).ilog2() as usize;
        for num_vars in one_chunk - 1..=many_chunks {
            let size = 1usize << num_vars;
            let weights = random_weights(&mut rng, size - 3);
            let channels = [
                random_bytes(&mut rng, size),
                random_bytes(&mut rng, size - size / 4 - 1),
                random_bytes(&mut rng, size / 2 + 1),
            ];
            assert_matches_hyperplonk(num_vars, &weights, channels.each_ref().map(Vec::as_slice));
        }
    }

    #[test]
    fn inputs_longer_than_the_hypercube_are_refused() {
        let mut rng = StdRng::seed_from_u64(15);
        let weights = random_weights(&mut rng, 9);
        let bytes = random_bytes(&mut rng, 9);
        let mut transcript = IOPTranscript::<F>::new(b"products");
        assert!(prove_products(3, &weights, [&bytes[..8]; 3], &mut transcript).is_err());
        assert!(prove_products(
            3,
            &weights[..8],
            [&bytes[..8], &bytes, &bytes[..8]],
            &mut transcript
        )
        .is_err());
        assert!(prove_products(0, &weights[..1], [&bytes[..1]; 3], &mut transcript).is_err());
    }

    /// Short slices fit any number of variables, so the number itself has a
    /// bound: past it the function returns before it sizes its tables.
    #[test]
    fn more_variables_than_the_proof_has_are_refused() {
        let mut rng = StdRng::seed_from_u64(16);
        let weights = random_weights(&mut rng, 8);
        let bytes = random_bytes(&mut rng, 8);
        let mut transcript = IOPTranscript::<F>::new(b"products");
        for num_vars in [NUM_VARS + 1, 40, 63, 64, usize::MAX] {
            assert!(
                prove_products(num_vars, &weights, [&bytes; 3], &mut transcript).is_err(),
                "{num_vars} variables"
            );
            assert!(
                prove_products(num_vars, &[], [&[]; 3], &mut transcript).is_err(),
                "{num_vars} variables, no values"
            );
        }
    }
}
