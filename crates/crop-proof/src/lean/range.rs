//! The range check: a HyperPlonk lookup, proved as a product check that ends
//! in a zero check. Pixels and counts stay bytes, and what depends on them
//! is computed when it is read. Only the fraction and the product are full
//! tables of field elements, and both are functions of the bytes and two
//! challenges. Each is built when it is needed and dropped after: one at a
//! time to commit to it, both for the zero check, and one at a time again
//! when the openings read them. Building a table is much cheaper than
//! holding the tables of three channels until the openings.

use std::collections::BTreeMap;

use anyhow::{ensure, Result};
use ark_bls12_381::Bls12_381;
use ark_ec::CurveGroup;
use ark_ff::{batch_inversion, Field, One, Zero};
use rayon::prelude::*;
use subroutines::pcs::prelude::Commitment;
use transcript::IOPTranscript;

use super::{
    eq, msm,
    open::Source,
    srs::Srs,
    sumcheck::{self, Rounds},
    Values,
};
use crate::{
    crop::append,
    iop::{self, challenge, check},
    wire::Sumcheck,
    F,
};

/// How many values a pixel or a count can take.
const VALUES: usize = 256;

/// Rounds of the zero check that read the original data instead of folded
/// tables. The folded tables are `2^LAZY_ROUNDS` times smaller than the
/// polynomials when they are first stored: 20 MiB at 20 variables. Each lazy
/// round costs about one pass over the fraction and the product.
const LAZY_ROUNDS: usize = 3;

/// Stretches of a table shorter than this stay on one thread.
const MIN_PARALLEL_ROWS: usize = 1 << 10;

pub(crate) struct RangeCheck<'a> {
    pub sumcheck: Sumcheck,
    counts_commitment: Commitment<Bls12_381>,
    product_commitment: Commitment<Bls12_381>,
    fraction_commitment: Commitment<Bls12_381>,
    /// The counts, and what builds the product and the fraction again when
    /// they are opened.
    lookup: Lookup<'a>,
}

impl RangeCheck<'_> {
    /// The three commitments in the order the proof lists them, each with
    /// the polynomial an opening of it reads. The prover takes both from
    /// here, so its list of polynomials cannot fall out of step with its
    /// list of commitments.
    pub(crate) fn committed(&self) -> [(Commitment<Bls12_381>, Committed<'_>); 3] {
        let read_as = |polynomial| Committed {
            lookup: &self.lookup,
            polynomial,
        };
        [
            (self.counts_commitment, read_as(Polynomial::Counts)),
            (self.product_commitment, read_as(Polynomial::Product)),
            (self.fraction_commitment, read_as(Polynomial::Fraction)),
        ]
    }
}

/// Proves that every pixel of one channel is in 0..=255, as
/// `reference::prove_range` does for an image over `num_vars` variables,
/// with the same transcript. The range polynomials have one variable more.
pub(crate) fn prove_range<'a>(
    srs: &Srs,
    num_vars: usize,
    pixels: &'a [u8],
    transcript: &mut IOPTranscript<F>,
) -> Result<RangeCheck<'a>> {
    prove(srs, num_vars, pixels, LAZY_ROUNDS, transcript)
}

fn prove<'a>(
    srs: &Srs,
    num_vars: usize,
    pixels: &'a [u8],
    lazy_rounds: usize,
    transcript: &mut IOPTranscript<F>,
) -> Result<RangeCheck<'a>> {
    ensure!(
        (2..iop::IRREDUCIBLE.len() - 1).contains(&num_vars),
        "no range table for images over {num_vars} variables"
    );
    ensure!(
        num_vars < srs.num_vars(),
        "the commitment key is too small for a range check over {num_vars} variables"
    );
    ensure!(
        pixels.len() <= 1 << num_vars,
        "more pixels than the polynomial holds"
    );
    let basis = srs.basis(num_vars + 1);
    let (counts, shifted_counts) = sorted_counts(num_vars, pixels);

    // H goes into the transcript before the challenges, so it cannot be chosen
    // after seeing them (attack A3).
    let counts_commitment = Commitment(msm::msm_bytes(&basis, &counts).into_affine());
    append(transcript, b"hCom(x)", &counts_commitment)?;
    let offset = challenge(transcript, b"alpha")?;
    let mix = challenge(transcript, b"alpha")?;

    let lookup = Lookup::new(num_vars, pixels, counts, shifted_counts, offset, mix)?;
    // Each table lives for its own commitment only: a commitment needs
    // about as much memory again as the table it commits to.
    let fraction_commitment = Commitment(msm::msm(&basis, &lookup.fraction()).into_affine());
    let product_commitment = Commitment(msm::msm(&basis, &lookup.product()).into_affine());
    append(transcript, b"frac(x)", &fraction_commitment)?;
    append(transcript, b"prod(x)", &product_commitment)?;
    let alpha = challenge(transcript, b"alpha")?;

    let point = check(
        transcript.get_and_append_challenge_vectors(b"0check r", num_vars + 1),
        "deriving the zero check point",
    )?;
    let (fraction, product) = (lookup.fraction(), lookup.product());
    let mut rounds = ZeroCheck::new(&lookup, &fraction, &product, alpha, point, lazy_rounds);
    let sumcheck = sumcheck::prove(&mut rounds, transcript)?;
    Ok(RangeCheck {
        sumcheck,
        counts_commitment,
        product_commitment,
        fraction_commitment,
        lookup,
    })
}

/// `iop::sorted_counts` in bytes, for an image over `num_vars` variables:
/// along the walk through GF(2^(num_vars + 1)), each value 0..=255 once per
/// occurrence among the pixels plus one, and the same one slot later.
fn sorted_counts(num_vars: usize, pixels: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let size = 1u64 << (num_vars + 1);
    let reduction = iop::IRREDUCIBLE[num_vars + 1] - size;
    let mut occurrences = [0usize; VALUES];
    for &pixel in pixels {
        occurrences[usize::from(pixel)] += 1;
    }
    let mut counts = vec![0u8; size as usize];
    let mut shifted = vec![0u8; size as usize];
    let mut slot = 1;
    for (value, &occurrence) in (0..=u8::MAX).zip(&occurrences) {
        for _ in 0..=occurrence {
            counts[slot as usize] = value;
            slot = iop::times_x(slot, size, reduction);
            shifted[slot as usize] = value;
        }
    }
    (counts, shifted)
}

/// `step * value` for every byte value, by additions.
fn multiples(step: F) -> [F; VALUES] {
    let mut multiple = F::zero();
    std::array::from_fn(|_| {
        let current = multiple;
        multiple += step;
        current
    })
}

/// The two sides of the product check, read from bytes and never stored:
///
/// - f = (channel | table) + offset + mix * (channel | shifted table)
/// - g = counts + offset + mix * shifted counts
///
/// In its second half f is `offset` everywhere but on the slots of the
/// table walk.
///
/// The fraction and the product are functions of these bytes and the two
/// challenges, so this is also all it takes to build them, any number of
/// times. `fraction` and `product` are the only code that does, so a table
/// that is opened is the table that was committed to.
struct Lookup<'a> {
    /// Size of the image polynomial, half of the range polynomials.
    half: usize,
    pixels: &'a [u8],
    counts: Vec<u8>,
    shifted_counts: Vec<u8>,
    offset: F,
    mix: F,
    /// `(x, f(x) - offset)` where the tables are not zero, in order of x.
    walk: Vec<(usize, F)>,
}

/// f / g at single points of the hypercube.
struct Ratio<'a> {
    lookup: &'a Lookup<'a>,
    /// 1 / g for every pair of a count and a shifted count.
    inverses: Vec<F>,
    /// f for every pixel value.
    numerators: [F; VALUES],
}

impl Ratio<'_> {
    fn inverse(&self, x: usize) -> F {
        self.inverses[self.lookup.pair(x)]
    }

    /// f(x) / g(x) for an x that is not a slot of the table walk.
    fn off_walk(&self, x: usize) -> F {
        let lookup = self.lookup;
        let numerator = lookup
            .pixels
            .get(x)
            .map_or(lookup.offset, |&pixel| self.numerators[usize::from(pixel)]);
        numerator * self.inverse(x)
    }

    /// f(x) / g(x) for any x.
    fn at(&self, x: usize) -> F {
        let lookup = self.lookup;
        match lookup.walk.binary_search_by_key(&x, |&(slot, _)| slot) {
            Ok(step) => (lookup.offset + lookup.walk[step].1) * self.inverse(x),
            Err(_) => self.off_walk(x),
        }
    }
}

impl<'a> Lookup<'a> {
    /// Fails if g is zero somewhere, as hyperplonk does when it builds the
    /// fraction. Checking here leaves nothing that can fail when the tables
    /// are built, the first time or again.
    fn new(
        num_vars: usize,
        pixels: &'a [u8],
        counts: Vec<u8>,
        shifted_counts: Vec<u8>,
        offset: F,
        mix: F,
    ) -> Result<Self> {
        let half = 1 << num_vars;
        // Later values overwrite earlier ones when the walk wraps around, as
        // they do in the tables of `iop::range_tables`. It wraps under 8
        // variables only.
        let mut table = BTreeMap::<usize, (u8, u8)>::new();
        let slots = iop::range_table_slots(num_vars);
        for (value, pair) in (1..=u8::MAX).zip(slots.windows(2)) {
            table.entry(pair[0]).or_default().0 = value;
            table.entry(pair[1]).or_default().1 = value;
        }
        let walk = table
            .into_iter()
            .map(|(slot, (value, shifted))| (half + slot, F::from(value) + mix * F::from(shifted)))
            .collect();
        let lookup = Self {
            half,
            pixels,
            counts,
            shifted_counts,
            offset,
            mix,
            walk,
        };
        let denominators = lookup.denominators();
        ensure!(
            !(0..2 * half)
                .into_par_iter()
                .any(|x| denominators[lookup.pair(x)].is_zero()),
            "range product check: a denominator is zero"
        );
        Ok(lookup)
    }

    /// Where the count and the shifted count at x put g(x) in
    /// `denominators`.
    fn pair(&self, x: usize) -> usize {
        usize::from(self.counts[x]) << 8 | usize::from(self.shifted_counts[x])
    }

    /// g for every pair of a count and a shifted count: g depends on these
    /// two bytes only.
    fn denominators(&self) -> Vec<F> {
        let by_count = multiples(F::one());
        let by_shifted = multiples(self.mix);
        by_count
            .iter()
            .flat_map(|count| by_shifted.iter().map(move |shifted| (count, shifted)))
            .map(|(count, shifted)| self.offset + count + shifted)
            .collect()
    }

    fn ratio(&self) -> Ratio<'_> {
        // One inversion for all the values g can take. `new` saw that g is
        // not zero on the hypercube.
        let mut inverses = self.denominators();
        batch_inversion(&mut inverses);
        Ratio {
            lookup: self,
            inverses,
            numerators: multiples(F::one() + self.mix).map(|multiple| self.offset + multiple),
        }
    }

    /// f / g over the hypercube.
    fn fraction(&self) -> Vec<F> {
        let ratio = self.ratio();
        let mut fraction: Vec<F> = (0..2 * self.half)
            .into_par_iter()
            .map(|x| ratio.off_walk(x))
            .collect();
        for &(x, _) in &self.walk {
            fraction[x] = ratio.at(x);
        }
        fraction
    }

    /// hyperplonk's `compute_product_poly` of the fraction: entry x is the
    /// product of entries 2x and 2x + 1 of (fraction | product), and the last
    /// entry is zero. The first half reads the fraction, which is computed
    /// for it and not stored. Each later stretch of entries reads the
    /// stretch before it.
    fn product(&self) -> Vec<F> {
        let ratio = self.ratio();
        let mut product = vec![F::zero(); 2 * self.half];
        let mut length = self.half;
        product[..length]
            .par_iter_mut()
            .enumerate()
            .for_each(|(x, product)| *product = ratio.off_walk(2 * x) * ratio.off_walk(2 * x + 1));
        for &(x, _) in &self.walk {
            product[x / 2] = ratio.at(x & !1) * ratio.at(x | 1);
        }
        let mut start = 0;
        while length > 1 {
            let (read, write) = product[start..].split_at_mut(length);
            write[..length / 2]
                .par_iter_mut()
                .zip(read.par_chunks_exact(2))
                .with_min_len(MIN_PARALLEL_ROWS)
                .for_each(|(product, pair)| *product = pair[0] * pair[1]);
            start += length;
            length /= 2;
        }
        product
    }
}

/// A polynomial a range check committed to, as an opening reads it. The
/// counts are bytes the range check holds. The product and the fraction are
/// built for the read and freed after it.
pub(crate) struct Committed<'a> {
    lookup: &'a Lookup<'a>,
    polynomial: Polynomial,
}

enum Polynomial {
    Counts,
    Product,
    Fraction,
}

impl Source for Committed<'_> {
    fn read(&self, read: &mut dyn FnMut(Values<'_>)) {
        match self.polynomial {
            Polynomial::Counts => read(Values::Bytes(&self.lookup.counts)),
            Polynomial::Product => read(Values::Field(&self.lookup.product())),
            Polynomial::Fraction => read(Values::Field(&self.lookup.fraction())),
        }
    }
}

/// The five tables of the zero check at one index. With p1 and p2 the even
/// and the odd entries of (fraction | product), the zero check sums
///
///   eq * (product - p1 * p2 + alpha * (g * fraction - f))
///
/// Folding is linear, so `product - alpha * f` is one table, and `g` holds
/// `alpha * g`.
#[derive(Clone, Copy)]
struct Row {
    linear: F,
    p1: F,
    p2: F,
    g: F,
    fraction: F,
}

impl Row {
    /// The row at `r` on the line through `self` at 0 and `next` at 1.
    fn fold(&self, next: &Row, r: F) -> Row {
        let line = |from: F, to: F| from + (to - from) * r;
        Row {
            linear: line(self.linear, next.linear),
            p1: line(self.p1, next.p1),
            p2: line(self.p2, next.p2),
            g: line(self.g, next.g),
            fraction: line(self.fraction, next.fraction),
        }
    }

    fn value(&self) -> F {
        self.linear - self.p1 * self.p2 + self.g * self.fraction
    }

    /// Along the line through `self` at 0 and `next` at 1 the summand has
    /// degree 2. Returns its values at 0 and at 1 and its coefficient of X^2.
    fn terms(&self, next: &Row) -> [F; 3] {
        [
            self.value(),
            next.value(),
            (next.g - self.g) * (next.fraction - self.fraction)
                - (next.p1 - self.p1) * (next.p2 - self.p2),
        ]
    }
}

/// Fixes the lowest variable of `rows` to `r` in the same buffer.
fn fold_in_place(rows: &mut Vec<Row>, r: F) {
    let half = rows.len() / 2;
    if half == 0 {
        return;
    }
    // Rows start..2 * start of the result come from rows 2 * start..4 * start,
    // which lie past them, so each such band is written in parallel.
    rows[0] = rows[0].fold(&rows[1], r);
    let mut start = 1;
    while start < half {
        let (folded, rest) = rows.split_at_mut(2 * start);
        folded[start..]
            .par_iter_mut()
            .zip(rest[..2 * start].par_chunks_exact(2))
            .with_min_len(MIN_PARALLEL_ROWS)
            .for_each(|(row, pair)| *row = pair[0].fold(&pair[1], r));
        start *= 2;
    }
    rows.truncate(half);
}

/// `sum of weights[j] * values[j * step]`, for weights that sum to one.
fn weighted(weights: &[F], values: &[F], step: usize) -> F {
    let first = values[0];
    weights
        .iter()
        .zip(values.iter().step_by(step))
        .skip(1)
        .fold(first, |sum, (weight, value)| {
            sum + *weight * (*value - first)
        })
}

/// What it takes to read the tables as they are after some challenges
/// without storing them: a folded entry is a sum of `2^vars` original
/// entries, each times a weight.
struct Level {
    vars: usize,
    /// eq(challenges, j): the weight of entry j of each block.
    weights: Vec<F>,
    /// `alpha * offset`, which every entry of `alpha * f` and `alpha * g`
    /// holds whatever the challenges.
    constant: F,
    /// Per weight and byte: `alpha * weight * byte`, which is what a count
    /// adds to `alpha * g`.
    counts: Vec<[F; VALUES]>,
    /// The same for a shifted count: `alpha * mix * weight * byte`.
    shifted_counts: Vec<[F; VALUES]>,
    /// The same for a pixel in `alpha * f`: `alpha * (1 + mix) * weight * byte`.
    pixels: Vec<[F; VALUES]>,
    /// `alpha * (f - offset)` over the table walk, folded: `(index, value)`.
    walk: Vec<(usize, F)>,
}

impl Level {
    fn new(lookup: &Lookup, alpha: F, challenges: &[F]) -> Self {
        let vars = challenges.len();
        let weights = eq::table(challenges);
        let scaled = |by: F| {
            weights
                .iter()
                .map(|weight| multiples(by * weight))
                .collect()
        };
        let mut walk = Vec::<(usize, F)>::with_capacity(lookup.walk.len());
        for &(x, value) in &lookup.walk {
            let index = x >> vars;
            let value = alpha * weights[x & (weights.len() - 1)] * value;
            match walk.last_mut() {
                Some(last) if last.0 == index => last.1 += value,
                _ => walk.push((index, value)),
            }
        }
        Self {
            vars,
            constant: alpha * lookup.offset,
            counts: scaled(alpha),
            shifted_counts: scaled(alpha * lookup.mix),
            pixels: scaled(alpha * (F::one() + lookup.mix)),
            weights,
            walk,
        }
    }
}

/// eq(x, point) as `low[x mod 2^k] * high[x div 2^k]`.
struct RestEq {
    low: Vec<F>,
    high: Vec<F>,
    low_vars: usize,
}

impl RestEq {
    fn new(point: &[F]) -> Self {
        let low_vars = point.len() / 2;
        Self {
            low: eq::table(&point[..low_vars]),
            high: eq::table(&point[low_vars..]),
            low_vars,
        }
    }

    fn at(&self, x: usize) -> F {
        self.low[x & (self.low.len() - 1)] * self.high[x >> self.low_vars]
    }

    /// `sum over x of eq(x, point) * terms(x)`, one sum per term.
    fn sum<const N: usize>(&self, terms: impl Fn(usize) -> [F; N] + Sync) -> [F; N] {
        let add = |mut sums: [F; N], terms: [F; N]| {
            for (sum, term) in sums.iter_mut().zip(terms) {
                *sum += term;
            }
            sums
        };
        self.high
            .par_iter()
            .enumerate()
            .map(|(high_index, high)| {
                let base = high_index << self.low_vars;
                let sums =
                    self.low
                        .iter()
                        .enumerate()
                        .fold([F::zero(); N], |sums, (low_index, low)| {
                            add(sums, terms(base | low_index).map(|term| term * low))
                        });
                sums.map(|sum| sum * high)
            })
            .reduce(|| [F::zero(); N], add)
    }
}

/// The sumcheck of the zero check. With z the zero check's point and
/// r_0..r_(k-1) the challenges so far, round k sends
///
///   eq(r, z_0..z_(k-1)) * eq(X, z_k) * t(X)
///   t(X) = sum over b of eq(b, z_(k+1)..) * summand(r, X, b)
///
/// The eq factor is never a table: a scalar, a line, and an outer product
/// of two small tables. t has degree 2.
///
/// The fraction and the product must be the ones built from the lookup: the
/// first round relies on the summand being zero on the hypercube.
struct ZeroCheck<'a> {
    lookup: &'a Lookup<'a>,
    fraction: &'a [F],
    product: &'a [F],
    alpha: F,
    point: Vec<F>,
    challenges: Vec<F>,
    /// eq(challenges, point[..challenges.len()]).
    scalar: F,
    lazy_rounds: usize,
    /// The folded tables, from the round after the lazy ones on.
    rows: Option<Vec<Row>>,
}

impl<'a> ZeroCheck<'a> {
    fn new(
        lookup: &'a Lookup<'a>,
        fraction: &'a [F],
        product: &'a [F],
        alpha: F,
        point: Vec<F>,
        lazy_rounds: usize,
    ) -> Self {
        Self {
            lookup,
            fraction,
            product,
            alpha,
            challenges: Vec::with_capacity(point.len()),
            point,
            scalar: F::one(),
            lazy_rounds,
            rows: None,
        }
    }

    /// (fraction | product) from entry `2 * x` to the end of its half: p1
    /// from x on in the even places and p2 in the odd ones.
    fn pairs(&self, x: usize) -> &[F] {
        match x.checked_sub(self.lookup.half) {
            None => &self.fraction[2 * x..],
            Some(x) => &self.product[2 * x..],
        }
    }

    /// The tables at `index` after the level's challenges, but for the table
    /// walk in f, which `Level::walk` holds.
    fn row(&self, level: &Level, index: usize) -> Row {
        let lookup = self.lookup;
        let start = index << level.vars;
        let pairs = self.pairs(start);
        let g = lookup.counts[start..]
            .iter()
            .zip(&lookup.shifted_counts[start..])
            .zip(level.counts.iter().zip(&level.shifted_counts))
            .fold(
                level.constant,
                |sum, ((count, shifted), (by_count, by_shifted))| {
                    sum + by_count[usize::from(*count)] + by_shifted[usize::from(*shifted)]
                },
            );
        let f = lookup
            .pixels
            .get(start..)
            .unwrap_or_default()
            .iter()
            .zip(&level.pixels)
            .fold(level.constant, |sum, (pixel, by_pixel)| {
                sum + by_pixel[usize::from(*pixel)]
            });
        Row {
            linear: weighted(&level.weights, &self.product[start..], 1) - f,
            p1: weighted(&level.weights, pairs, 2),
            p2: weighted(&level.weights, &pairs[1..], 2),
            g,
            fraction: weighted(&level.weights, &self.fraction[start..], 1),
        }
    }

    /// The tables after the challenges so far.
    fn store(&self) -> Vec<Row> {
        let level = Level::new(self.lookup, self.alpha, &self.challenges);
        let mut rows: Vec<Row> = (0..self.fraction.len() >> level.vars)
            .into_par_iter()
            .map(|index| self.row(&level, index))
            .collect();
        for &(index, value) in &level.walk {
            rows[index].linear -= value;
        }
        rows
    }

    /// t at 0 and 1 and its coefficient of X^2, from the original data.
    fn lazy_terms(&self, eq: &RestEq) -> [F; 3] {
        let level = Level::new(self.lookup, self.alpha, &self.challenges);
        if level.vars > 0 {
            let mut terms = eq.sum(|b| self.row(&level, 2 * b).terms(&self.row(&level, 2 * b + 1)));
            for &(index, value) in &level.walk {
                terms[index & 1] -= eq.at(index >> 1) * value;
            }
            return terms;
        }
        // The summand is zero on the hypercube: the product and the fraction
        // are built to make it so. In the first round t is therefore zero at
        // 0 and at 1, and only the products of two tables reach X^2.
        let lookup = self.lookup;
        let (by_count, by_shifted) = (&level.counts[0], &level.shifted_counts[0]);
        let byte = |bytes: &[u8], x: usize| usize::from(bytes[x]);
        let [square] = eq.sum(|b| {
            let (x, next) = (2 * b, 2 * b + 1);
            let pairs = self.pairs(x);
            let g = by_count[byte(&lookup.counts, next)] - by_count[byte(&lookup.counts, x)]
                + by_shifted[byte(&lookup.shifted_counts, next)]
                - by_shifted[byte(&lookup.shifted_counts, x)];
            [g * (self.fraction[next] - self.fraction[x])
                - (pairs[2] - pairs[0]) * (pairs[3] - pairs[1])]
        });
        [F::zero(), F::zero(), square]
    }
}

impl Rounds for ZeroCheck<'_> {
    fn num_vars(&self) -> usize {
        self.point.len()
    }

    fn max_degree(&self) -> usize {
        3
    }

    fn round(&mut self, challenge: Option<F>) -> Vec<F> {
        if let Some(r) = challenge {
            let z = self.point[self.challenges.len()];
            self.scalar *= r * z + (F::one() - r) * (F::one() - z);
            self.challenges.push(r);
            if let Some(rows) = &mut self.rows {
                fold_in_place(rows, r);
            }
        }
        if self.rows.is_none() && self.challenges.len() >= self.lazy_rounds {
            self.rows = Some(self.store());
        }
        let round = self.challenges.len();
        let eq = RestEq::new(&self.point[round + 1..]);
        let [at_zero, at_one, square] = match &self.rows {
            Some(rows) => eq.sum(|b| rows[2 * b].terms(&rows[2 * b + 1])),
            None => self.lazy_terms(&eq),
        };
        let at_two = at_one.double() - at_zero + square.double();
        let at_three = at_zero + (at_two - at_one) * F::from(3u8);

        // eq(X, z) is a line: 1 - z at 0 and z at 1.
        let z = self.point[round];
        let step = z.double() - F::one();
        let mut line = F::one() - z;
        [at_zero, at_one, at_two, at_three]
            .into_iter()
            .map(|t| {
                let value = self.scalar * line * t;
                line += step;
                value
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arithmetic::merge_polynomials;
    use ark_ff::UniformRand;
    use ark_poly::DenseMultilinearExtension;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use subroutines::{
        pcs::prelude::{MultilinearProverParam, PolynomialCommitmentScheme},
        poly_iop::{prelude::ProductCheck, PolyIOP},
    };

    use super::*;
    use crate::{
        crop::new_transcript,
        iop::{field_values, poly, Poly},
        reference::ProductCheckProof,
        Pcs, NUM_VARS,
    };

    fn prover_key(num_vars: usize) -> MultilinearProverParam<Bls12_381> {
        let mut rng = StdRng::seed_from_u64(num_vars as u64);
        let srs = Pcs::gen_srs_for_testing(&mut rng, num_vars + 1).unwrap();
        Pcs::trim(&srs, None, Some(num_vars + 1)).unwrap().0
    }

    /// `iop::range_tables` for an image over `num_vars` variables.
    fn range_tables(num_vars: usize) -> (Vec<F>, Vec<F>) {
        let slots = iop::range_table_slots(num_vars);
        let mut table = vec![F::zero(); 1 << num_vars];
        let mut shifted = vec![F::zero(); 1 << num_vars];
        for (value, pair) in (1..=255u32).zip(slots.windows(2)) {
            table[pair[0]] = F::from(value);
            shifted[pair[1]] = F::from(value);
        }
        (table, shifted)
    }

    /// `iop::sorted_counts` for an image over `num_vars` variables.
    fn field_counts(num_vars: usize, pixels: &[u8]) -> (Vec<F>, Vec<F>) {
        let size = 1u64 << (num_vars + 1);
        let reduction = iop::IRREDUCIBLE[num_vars + 1] - size;
        let mut occurrences = [0usize; 256];
        for &pixel in pixels {
            occurrences[pixel as usize] += 1;
        }
        let mut counts = vec![F::zero(); size as usize];
        let mut shifted = vec![F::zero(); size as usize];
        let mut slot = 1;
        for (value, &occurrence) in occurrences.iter().enumerate() {
            for _ in 0..=occurrence {
                counts[slot as usize] = F::from(value as u32);
                slot = iop::times_x(slot, size, reduction);
                shifted[slot as usize] = F::from(value as u32);
            }
        }
        (counts, shifted)
    }

    fn scaled(polynomial: &Poly, by: F) -> DenseMultilinearExtension<F> {
        let values = polynomial
            .evaluations
            .iter()
            .map(|value| *value * by)
            .collect();
        DenseMultilinearExtension::from_evaluations_vec(polynomial.num_vars, values)
    }

    /// f and g of the product check as full polynomials, and the counts,
    /// built as `reference::prove_range` builds them.
    fn sides(num_vars: usize, pixels: &[u8], offset: F, mix: F) -> (Poly, Poly, Poly) {
        let mut channel = field_values(pixels);
        channel.resize(1 << num_vars, F::zero());
        let channel = poly(channel);
        let (table, shifted_table) = range_tables(num_vars);
        let (counts, shifted_counts) = field_counts(num_vars, pixels);
        let left = [table, shifted_table]
            .map(|table| merge_polynomials(&[channel.clone(), poly(table)]).unwrap());
        let right = [poly(counts), poly(shifted_counts)];
        let constant = DenseMultilinearExtension::from_evaluations_vec(
            num_vars + 1,
            vec![offset; 1 << (num_vars + 1)],
        );
        let f = &(left[0].as_ref() + &constant) + &scaled(&left[1], mix);
        let g = &(right[0].as_ref() + &constant) + &scaled(&right[1], mix);
        (Arc::new(f), Arc::new(g), right[0].clone())
    }

    struct Reference {
        proof: ProductCheckProof,
        counts: Poly,
        counts_commitment: Commitment<Bls12_381>,
        product: Poly,
        fraction: Poly,
    }

    /// `reference::prove_range` for an image over `num_vars` variables.
    fn hyperplonk_range(
        params: &MultilinearProverParam<Bls12_381>,
        num_vars: usize,
        pixels: &[u8],
        transcript: &mut IOPTranscript<F>,
    ) -> Reference {
        let (counts, _) = field_counts(num_vars, pixels);
        let counts_commitment = Pcs::commit(params, &poly(counts)).unwrap();
        transcript
            .append_serializable_element(b"hCom(x)", &counts_commitment)
            .unwrap();
        let offset = transcript.get_and_append_challenge(b"alpha").unwrap();
        let mix = transcript.get_and_append_challenge(b"alpha").unwrap();
        let (f, g, counts) = sides(num_vars, pixels, offset, mix);
        let (proof, product, fraction) =
            <PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::prove(params, &[f], &[g], transcript)
                .unwrap();
        Reference {
            proof,
            counts,
            counts_commitment,
            product,
            fraction,
        }
    }

    fn inputs(num_vars: usize, rng: &mut StdRng) -> Vec<Vec<u8>> {
        let size = 1 << num_vars;
        let mut random = |length: usize| (0..length).map(|_| rng.gen()).collect::<Vec<u8>>();
        vec![
            random(size),
            random(size - 3),
            random(size / 2 + 1),
            random(1),
            Vec::new(),
            vec![0; size],
            vec![255; size],
        ]
    }

    fn assert_matches_hyperplonk(
        params: &MultilinearProverParam<Bls12_381>,
        srs: &Srs,
        num_vars: usize,
        pixels: &[u8],
        lazy_rounds: usize,
    ) {
        let case = format!(
            "{num_vars} variables, {} pixels, {lazy_rounds} lazy rounds",
            pixels.len()
        );
        let mut expected_transcript = new_transcript(&[7; 32]).unwrap();
        let expected = hyperplonk_range(params, num_vars, pixels, &mut expected_transcript);
        let mut transcript = new_transcript(&[7; 32]).unwrap();
        let range = prove(srs, num_vars, pixels, lazy_rounds, &mut transcript).unwrap();

        assert_eq!(
            field_values(&range.lookup.counts),
            expected.counts.evaluations
        );
        assert_eq!(range.counts_commitment, expected.counts_commitment);
        assert_eq!(range.fraction_commitment, expected.proof.frac_comm);
        assert_eq!(range.product_commitment, expected.proof.prod_x_comm);
        // The tables an opening will read are built again: the range check
        // has dropped the ones it committed to.
        assert_eq!(
            range.lookup.fraction(),
            expected.fraction.evaluations,
            "{case}"
        );
        assert_eq!(
            range.lookup.product(),
            expected.product.evaluations,
            "{case}"
        );
        assert_eq!(
            range.sumcheck,
            Sumcheck::from(expected.proof.zero_check_proof),
            "{case}"
        );
        assert_eq!(
            transcript.get_and_append_challenge(b"next").unwrap(),
            expected_transcript
                .get_and_append_challenge(b"next")
                .unwrap(),
            "{case}"
        );
    }

    #[test]
    fn range_check_matches_hyperplonk() {
        let mut rng = StdRng::seed_from_u64(19);
        // Every size `prove` accepts, up to one that is quick to check.
        for num_vars in 2..=10 {
            let params = prover_key(num_vars);
            let srs = Srs::Loaded(params.clone());
            for pixels in inputs(num_vars, &mut rng) {
                assert_matches_hyperplonk(&params, &srs, num_vars, &pixels, LAZY_ROUNDS);
            }
        }
    }

    #[test]
    fn any_number_of_lazy_rounds_gives_the_same_proof() {
        let mut rng = StdRng::seed_from_u64(20);
        // From tables stored before the first round to tables never stored:
        // at the smallest size, at one where the table walk wraps around, and
        // at one where the folds and the products run on several threads.
        for (num_vars, lazy_rounds) in [(2, 0..=4), (4, 0..=6), (7, 0..=9), (12, 0..=3)] {
            let params = prover_key(num_vars);
            let srs = Srs::Loaded(params.clone());
            let pixels: Vec<u8> = (0..(1 << num_vars) - 5).map(|_| rng.gen()).collect();
            for lazy_rounds in lazy_rounds {
                assert_matches_hyperplonk(&params, &srs, num_vars, &pixels, lazy_rounds);
            }
        }
    }

    #[test]
    fn a_larger_key_gives_the_same_proof() {
        let mut rng = StdRng::seed_from_u64(23);
        let num_vars = 6;
        let pixels: Vec<u8> = (0..(1 << num_vars) - 5).map(|_| rng.gen()).collect();
        for spare_vars in 1..=2 {
            let params = prover_key(num_vars + spare_vars);
            let srs = Srs::Loaded(params.clone());
            assert_matches_hyperplonk(&params, &srs, num_vars, &pixels, LAZY_ROUNDS);
        }
    }

    /// The tests above feed hyperplonk with copies of `iop::sorted_counts`
    /// and `iop::range_tables` for any size. At the size of the proof, the
    /// bytes this prover reads and those copies must be the originals.
    #[test]
    fn bytes_match_the_tables_of_the_proof() {
        let mut rng = StdRng::seed_from_u64(22);
        let size = 1 << NUM_VARS;
        let field: Vec<F> = (0..=u8::MAX).map(F::from).collect();
        let same = |bytes: &[u8], values: &[F]| {
            bytes
                .iter()
                .map(|&byte| field[usize::from(byte)])
                .eq(values.iter().copied())
        };
        for length in [size, size - 12345, 1, 0] {
            let pixels: Vec<u8> = (0..length).map(|_| rng.gen()).collect();
            let (counts, shifted_counts) = sorted_counts(NUM_VARS, &pixels);
            let expected = iop::sorted_counts(&pixels);
            assert!(same(&counts, &expected.0), "{length} pixels");
            assert!(same(&shifted_counts, &expected.1), "{length} pixels");
            assert!(
                field_counts(NUM_VARS, &pixels) == expected,
                "{length} pixels"
            );
        }

        let (table, shifted_table) = iop::range_tables();
        let copies = range_tables(NUM_VARS);
        assert!(copies.0 == table && copies.1 == shifted_table);
        let (offset, mix) = (F::rand(&mut rng), F::rand(&mut rng));
        let (counts, shifted_counts) = sorted_counts(NUM_VARS, &[]);
        let lookup = Lookup::new(NUM_VARS, &[], counts, shifted_counts, offset, mix).unwrap();
        let expected: Vec<(usize, F)> = table
            .iter()
            .zip(&shifted_table)
            .enumerate()
            .filter(|(_, (value, shifted))| !value.is_zero() || !shifted.is_zero())
            .map(|(slot, (value, shifted))| (size + slot, *value + mix * shifted))
            .collect();
        assert_eq!(lookup.walk, expected);
    }

    #[test]
    fn a_zero_denominator_is_an_error_as_in_hyperplonk() {
        let num_vars = 5;
        let params = prover_key(num_vars);
        let mut rng = StdRng::seed_from_u64(21);
        let pixels: Vec<u8> = (0..1 << num_vars).map(|_| rng.gen()).collect();
        let (counts, shifted_counts) = sorted_counts(num_vars, &pixels);
        // g is zero where both counts are zero.
        let (offset, mix) = (F::zero(), F::rand(&mut rng));
        assert!(Lookup::new(num_vars, &pixels, counts, shifted_counts, offset, mix).is_err());

        let (f, g, _) = sides(num_vars, &pixels, offset, mix);
        let mut transcript = new_transcript(&[7; 32]).unwrap();
        assert!(<PolyIOP<F> as ProductCheck<Bls12_381, Pcs>>::prove(
            &params,
            &[f],
            &[g],
            &mut transcript
        )
        .is_err());
    }

    /// What an opening reads from a range check is what the range check
    /// committed to, at every read: `committed` pairs each commitment with
    /// its own polynomial, and lists them as the verifier takes them.
    #[test]
    fn every_read_gives_the_committed_polynomial() {
        let mut rng = StdRng::seed_from_u64(24);
        // A size where the table walk wraps around, one that stays on one
        // thread, and one where the tables are built on several.
        for num_vars in [4, 7, 12] {
            let params = prover_key(num_vars);
            let srs = Srs::Loaded(params.clone());
            let basis = srs.basis(num_vars + 1);
            for pixels in inputs(num_vars, &mut rng) {
                let mut transcript = new_transcript(&[7; 32]).unwrap();
                let range = prove_range(&srs, num_vars, &pixels, &mut transcript).unwrap();
                let committed = range.committed();
                // `crop::verify_prefix` reads the counts, the product and
                // the fraction of a channel in this order.
                assert_eq!(
                    committed.each_ref().map(|(commitment, _)| *commitment),
                    [
                        range.counts_commitment,
                        range.product_commitment,
                        range.fraction_commitment
                    ]
                );
                for (commitment, source) in &committed {
                    let mut reads = 0;
                    for _ in 0..2 {
                        source.read(&mut |values| {
                            assert_eq!(values.len(), 2 << num_vars);
                            let read = match values {
                                Values::Bytes(values) => msm::msm_bytes(&basis, values),
                                Values::Field(values) => msm::msm(&basis, values),
                            };
                            assert_eq!(Commitment(read.into_affine()), *commitment);
                            reads += 1;
                        });
                    }
                    assert_eq!(reads, 2);
                }
            }
        }
    }

    #[test]
    fn rejects_sizes_it_cannot_prove() {
        let params = prover_key(4);
        let srs = Srs::Loaded(params);
        let mut transcript = new_transcript(&[7; 32]).unwrap();
        assert!(prove_range(&srs, 4, &[0; 17], &mut transcript).is_err());
        assert!(prove_range(&srs, 5, &[0; 17], &mut transcript).is_err());
        assert!(prove_range(&srs, 1, &[0; 2], &mut transcript).is_err());
    }
}
