//! hyperplonk's batch opening (`multi_open`, then `open`) without its tables.
//!
//! One sumcheck over `sum of eq(x, point) * coefficient * polynomial(x)`,
//! taken over the openings, reduces them to one opening of a combination of
//! the polynomials at the sumcheck's point. hyperplonk builds two tables per
//! distinct point, one for the polynomials opened there and one for eq, and
//! then the table of the combination. Here the largest tables are the
//! polynomials themselves with a few variables already fixed:
//!
//! - The first rounds need each polynomial only with its high variables
//!   fixed to the opening point. That is a few values per opening, and one
//!   pass over the polynomial computes them. The same values give the
//!   evaluation the opening claims.
//! - After those rounds each polynomial is folded once, from its values, into
//!   a table that is half the size for every round already played. A later
//!   round takes inner products of these tables with eq, and eq is the outer
//!   product of two short tables.
//! - The final opening fixes the variables of the combination to the
//!   sumcheck's challenges, one per round, and commits to a quotient each
//!   time. The first quotients are computed from the polynomials. Only then
//!   is the combination stored, with those variables fixed.
//!
//! Each of these three steps reads one polynomial after the other, and what
//! it keeps of a polynomial is small. So the values of a polynomial need to
//! exist only while it is read: the tables of the range checks are built
//! for each read and dropped after it.

use std::collections::BTreeMap;

use anyhow::{ensure, Context, Result};
use ark_bls12_381::Bls12_381;
use ark_ec::CurveGroup;
use ark_ff::{One, Zero};
use rayon::prelude::*;
use subroutines::pcs::prelude::MultilinearKzgProof;
use transcript::IOPTranscript;

use super::{
    dense::Dense,
    eq::{self, SplitEq},
    msm,
    srs::Srs,
    sumcheck::{self, Rounds},
    Values,
};
use crate::{
    iop::{check, Opening},
    wire::BatchOpening,
    F,
};

/// Rounds of the sumcheck played before the polynomials are folded. The
/// folded tables are `2^LAZY_ROUNDS` times smaller than the polynomials. One
/// pass serves all these rounds, so more of them cost no time, only more sums
/// per thread while the pass runs: `256 * 2^LAZY_ROUNDS` field elements for a
/// polynomial of bytes. Measured on the range batch over 16 and 18
/// variables, 4 to 6 rounds took the same time within noise, and 5 keeps both
/// the tables and the sums small.
const LAZY_ROUNDS: usize = 5;

/// Rounds of the final opening whose quotient is computed from the
/// polynomials, into a table of its own. Each of them reads every polynomial
/// once more. With one such round, the largest table held next to the
/// polynomial being read is half its size: first that quotient, then the
/// combination with one variable fixed. With none it is the combination in
/// full. More rounds save nothing, because the first quotient stays the
/// largest table. Measured on the range batch over 20 variables, the live
/// heap of the opening peaks at 80 MB with no such round and at 64 MB with
/// one or with three.
const LAZY_QUOTIENTS: usize = 1;

/// Variables of the low eq table when the values are bytes. Sums are kept
/// per byte value and settled once per block, so a block must hold many more
/// values than there are byte values.
const BYTE_BLOCK_VARS: usize = 12;

/// A polynomial of a batch. The opening reads one source at a time and is
/// done with its values when the read returns, so a source can build its
/// values for each read.
pub(crate) trait Source {
    /// Calls `read` once with the values of the polynomial. Every call
    /// passes the same values. An opening refuses a source whose first read
    /// does not pass its values exactly once, and relies on the later reads
    /// to repeat the first.
    fn read(&self, read: &mut dyn FnMut(Values<'_>));
}

/// Values that are in memory anyway.
impl Source for Values<'_> {
    fn read(&self, read: &mut dyn FnMut(Values<'_>)) {
        read(*self);
    }
}

/// Opens `polynomials[opening.commitment]` at `points[opening.point]` for
/// every opening, with the transcript and the proof of hyperplonk's
/// `multi_open`. Every polynomial an opening names has `num_vars` variables.
pub(crate) fn open(
    srs: &Srs,
    num_vars: usize,
    polynomials: &[&dyn Source],
    points: &[Vec<F>],
    openings: &[Opening],
    transcript: &mut IOPTranscript<F>,
) -> Result<BatchOpening> {
    open_with(
        srs,
        num_vars,
        polynomials,
        points,
        openings,
        transcript,
        LAZY_ROUNDS,
        LAZY_QUOTIENTS,
    )
}

/// `open` with the numbers of lazy rounds as arguments, which do not change
/// the proof.
#[allow(clippy::too_many_arguments)]
fn open_with(
    srs: &Srs,
    num_vars: usize,
    polynomials: &[&dyn Source],
    points: &[Vec<F>],
    openings: &[Opening],
    transcript: &mut IOPTranscript<F>,
    lazy_rounds: usize,
    lazy_quotients: usize,
) -> Result<BatchOpening> {
    // hyperplonk rejects a batch of one: it has no batching point to draw.
    ensure!(
        openings.len() > 1,
        "a batch opening needs at least two openings"
    );
    ensure!(
        (1..=srs.num_vars()).contains(&num_vars),
        "the parameters do not cover {num_vars} variables"
    );
    for opening in openings {
        ensure!(
            opening.commitment < polynomials.len(),
            "no polynomial {} to open",
            opening.commitment
        );
        let point = points
            .get(opening.point)
            .with_context(|| format!("no point {} to open at", opening.point))?;
        ensure!(
            point.len() == num_vars,
            "point {} has {} coordinates, not {num_vars}",
            opening.point,
            point.len()
        );
    }
    let (source_ids, source_slots) = distinct(openings.iter().map(|opening| opening.commitment));
    let sources: Vec<&dyn Source> = source_ids.iter().map(|&id| polynomials[id]).collect();

    // One pass over a polynomial serves the evaluation and the lazy rounds
    // of every opening of it at points that agree in their high coordinates.
    let lazy_vars = lazy_rounds.clamp(1, num_vars);
    let (passes, pass_slots) = distinct(
        openings
            .iter()
            .zip(&source_slots)
            .map(|(opening, &source)| (source, &points[opening.point][lazy_vars..])),
    );
    let (pass_heads, lengths) = heads(&sources, &source_ids, num_vars, lazy_vars, &passes)?;
    let heads: Vec<&[F]> = pass_slots
        .iter()
        .map(|&slot| pass_heads[slot].as_slice())
        .collect();
    let evaluations: Vec<F> = openings
        .iter()
        .zip(&heads)
        .map(|(opening, head)| {
            opening.fixed.unwrap_or_else(|| {
                eq::table(&points[opening.point][..lazy_vars])
                    .iter()
                    .zip(*head)
                    .map(|(eq, value)| *eq * value)
                    .sum()
            })
        })
        .collect();

    for opening in openings {
        check(
            transcript.append_serializable_element(b"eval_point", &points[opening.point]),
            "appending to the transcript",
        )?;
    }
    for evaluation in &evaluations {
        check(
            transcript.append_serializable_element(b"eval", evaluation),
            "appending to the transcript",
        )?;
    }
    let t = check(
        transcript.get_and_append_challenge_vectors(b"t", ark_std::log2(openings.len()) as usize),
        "deriving the batching point",
    )?;
    let coefficients = eq::table(&t);

    let (point_ids, point_slots) = distinct(openings.iter().map(|opening| opening.point));
    let terms = coefficients
        .iter()
        .zip(source_slots.iter().zip(&point_slots))
        .map(|(&coefficient, (&source, &point))| Term {
            coefficient,
            source,
            point,
        })
        .collect();
    let mut batch = Batch::new(
        num_vars,
        &sources,
        point_ids.iter().map(|&id| points[id].as_slice()).collect(),
        terms,
        lazy_vars,
        &heads,
    );
    let sumcheck = sumcheck::prove(&mut batch, transcript)?;

    let combination = Combination {
        sources: &sources,
        weights: batch.weights(&sumcheck.point),
        // Short polynomials end in zeros, and so does their combination.
        live: lengths.iter().copied().max().unwrap_or(0),
    };
    // Its folded tables keep their allocations to the last round.
    drop(batch);
    let kzg = combination.open(srs, num_vars, &sumcheck.point, lazy_quotients);
    Ok(BatchOpening {
        sumcheck,
        evaluations,
        kzg,
    })
}

/// The distinct values of `ids` in order of first use, and for each id its
/// position among them.
fn distinct<T: Ord + Copy>(ids: impl Iterator<Item = T>) -> (Vec<T>, Vec<usize>) {
    let mut found = BTreeMap::new();
    let mut order = Vec::new();
    let slots = ids
        .map(|id| {
            *found.entry(id).or_insert_with(|| {
                order.push(id);
                order.len() - 1
            })
        })
        .collect();
    (order, slots)
}

/// Per pass `(source, tail)`, the source with every variable from `low_vars`
/// up fixed to `tail`; and per source, how many values it has.
///
/// This is the first read of every source, so it is where a source the batch
/// cannot open is refused: one with more than `2^num_vars` values, and one
/// that does not pass its values exactly once. Without its values a
/// polynomial would be opened as zero, in a proof that does not verify.
/// `ids` names the sources in these errors.
fn heads(
    sources: &[&dyn Source],
    ids: &[usize],
    num_vars: usize,
    low_vars: usize,
    passes: &[(usize, &[F])],
) -> Result<(Vec<Vec<F>>, Vec<usize>)> {
    let mut heads = vec![Vec::new(); passes.len()];
    let mut lengths = Vec::with_capacity(sources.len());
    for (source, (polynomial, id)) in sources.iter().zip(ids).enumerate() {
        let wanted: Vec<usize> = (0..passes.len())
            .filter(|&pass| passes[pass].0 == source)
            .collect();
        let (mut reads, mut length) = (0, 0);
        polynomial.read(&mut |values| {
            reads += 1;
            length = values.len();
            if length > 1 << num_vars {
                return;
            }
            let computed: Vec<Vec<F>> = wanted
                .par_iter()
                .map(|&pass| head(values, low_vars, passes[pass].1))
                .collect();
            for (&pass, head) in wanted.iter().zip(computed) {
                heads[pass] = head;
            }
        });
        ensure!(
            reads == 1,
            "polynomial {id} passed its values {reads} times, not once"
        );
        ensure!(
            length <= 1 << num_vars,
            "polynomial {id} has more than 2^{num_vars} values"
        );
        lengths.push(length);
    }
    Ok((heads, lengths))
}

/// `values` as a polynomial with every variable from `low_vars` up fixed to
/// `tail`: the `2^low_vars` values that remain.
fn head(values: Values<'_>, low_vars: usize, tail: &[F]) -> Vec<F> {
    let eq = match values {
        Values::Bytes(_) => SplitEq::with_low_vars(tail, BYTE_BLOCK_VARS),
        Values::Field(_) => SplitEq::new(tail),
    };
    fix_high(values, low_vars, &eq)
}

/// `sum over y of eq(y) * values[y * 2^low_vars + c]` for every `c` below
/// `2^low_vars`: the polynomial with its high variables fixed to the point
/// `eq` was built from. Values past the end of the slice are zero.
fn fix_high(values: Values<'_>, low_vars: usize, eq: &SplitEq) -> Vec<F> {
    let width = 1 << low_vars;
    match values {
        Values::Field(values) => eq.sum_rows(
            width,
            || (),
            |(), first, low, sums| {
                let rows = values.get(first << low_vars..).unwrap_or(&[]);
                for (row, weight) in rows.chunks(width).zip(low) {
                    for (sum, value) in sums.iter_mut().zip(row) {
                        *sum += *weight * value;
                    }
                }
            },
        ),
        // A byte times a weight is the weight added to the sum kept for that
        // byte value. One multiplication per byte value settles a block.
        Values::Bytes(values) => eq.sum_rows(
            width,
            || vec![F::zero(); width << 8],
            |buckets, first, low, sums| {
                let rows = values.get(first << low_vars..).unwrap_or(&[]);
                if rows.is_empty() {
                    return;
                }
                buckets.fill(F::zero());
                for (row, weight) in rows.chunks(width).zip(low) {
                    for (column, &byte) in row.iter().enumerate() {
                        buckets[column << 8 | usize::from(byte)] += weight;
                    }
                }
                for (sum, bucket) in sums.iter_mut().zip(buckets.chunks(1 << 8)) {
                    *sum = weighted_by_index(bucket);
                }
            },
        ),
    }
}

/// `sum of i * values[i]` by additions only. Going down from the last value,
/// the running sum holds the values from the current index up, and
/// `values[i]` is in the `i` running sums taken at indices `i` down to 1.
fn weighted_by_index(values: &[F]) -> F {
    let mut running = F::zero();
    let mut total = F::zero();
    for value in values.iter().skip(1).rev() {
        running += value;
        total += running;
    }
    total
}

/// One opening inside the sumcheck: its coefficient, and where its
/// polynomial and its point sit among the distinct ones.
struct Term {
    coefficient: F,
    source: usize,
    point: usize,
}

/// The sumcheck of the batch. Its round polynomial is, summed over the
/// distinct points, `eq of the fixed coordinates` times `eq of this round's
/// coordinate`, which is a line, times a second line: the combination of
/// the polynomials opened at the point, with the later variables fixed to
/// the point.
struct Batch<'a> {
    num_vars: usize,
    sources: &'a [&'a dyn Source],
    points: Vec<&'a [F]>,
    terms: Vec<Term>,
    /// The first rounds, over the polynomials with their high variables
    /// fixed: by then they are small tables and need no care.
    lazy: Dense,
    lazy_vars: usize,
    challenges: Vec<F>,
    /// Per point, eq of the challenges so far and its first coordinates.
    scalars: Vec<F>,
    /// Per source, its values with the low variables fixed to the
    /// challenges. Empty during the lazy rounds.
    tables: Vec<Vec<F>>,
}

impl<'a> Batch<'a> {
    /// `heads[i]` is the polynomial of `terms[i]` with the variables from
    /// `lazy_vars` up fixed to its point.
    fn new(
        num_vars: usize,
        sources: &'a [&'a dyn Source],
        points: Vec<&'a [F]>,
        terms: Vec<Term>,
        lazy_vars: usize,
        heads: &[&[F]],
    ) -> Self {
        // Over the low variables the claim is a sum of eq times a table per
        // point: the heads of the openings at that point, combined.
        let mut tables = vec![vec![F::zero(); 1 << lazy_vars]; points.len()];
        for (term, head) in terms.iter().zip(heads) {
            for (sum, value) in tables[term.point].iter_mut().zip(*head) {
                *sum += term.coefficient * value;
            }
        }
        tables.extend(points.iter().map(|point| eq::table(&point[..lazy_vars])));
        let products = (0..points.len())
            .map(|slot| (F::one(), vec![slot, points.len() + slot]))
            .collect();
        Self {
            num_vars,
            lazy: Dense::new(lazy_vars, tables, products),
            lazy_vars,
            challenges: Vec::with_capacity(num_vars),
            scalars: vec![F::one(); points.len()],
            tables: Vec::new(),
            sources,
            points,
            terms,
        }
    }

    /// The polynomial the batch reduces to at the sumcheck's point `a` is a
    /// sum of the sources, each times the sum of `eq(a, point) * coefficient`
    /// over its openings. Returns that weight per source.
    fn weights(&self, a: &[F]) -> Vec<F> {
        let at_points: Vec<F> = self.points.iter().map(|point| eq::eval(a, point)).collect();
        let mut weights = vec![F::zero(); self.sources.len()];
        for term in &self.terms {
            weights[term.source] += term.coefficient * at_points[term.point];
        }
        weights
    }
}

impl Rounds for Batch<'_> {
    fn num_vars(&self) -> usize {
        self.num_vars
    }

    fn max_degree(&self) -> usize {
        2
    }

    fn round(&mut self, challenge: Option<F>) -> Vec<F> {
        if let Some(r) = challenge {
            let fixed = self.challenges.len();
            for (scalar, point) in self.scalars.iter_mut().zip(&self.points) {
                *scalar *= eq::line(r, point[fixed]);
            }
            self.challenges.push(r);
        }
        let round = self.challenges.len();
        if round < self.lazy_vars {
            return self.lazy.round(challenge);
        }
        if round == self.lazy_vars {
            let len = 1 << (self.num_vars - round);
            self.tables = vec![Vec::new(); self.sources.len()];
            for (source, table) in self.sources.iter().zip(&mut self.tables) {
                source.read(&mut |values| {
                    *table = Folded::new(F::one(), values, &self.challenges).table(len);
                });
            }
        } else if let Some(r) = challenge {
            self.tables
                .par_iter_mut()
                .for_each(|table| fold_in_place(table, r));
        }

        // Per opening, the inner products of eq over the later variables
        // with the even and with the odd entries of its table.
        let eqs: Vec<SplitEq> = self
            .points
            .iter()
            .map(|point| SplitEq::new(&point[round + 1..]))
            .collect();
        let halves: Vec<Vec<F>> = self
            .terms
            .par_iter()
            .map(|term| {
                fix_high(
                    Values::Field(&self.tables[term.source]),
                    1,
                    &eqs[term.point],
                )
            })
            .collect();
        let mut lines = vec![[F::zero(); 2]; self.points.len()];
        for (term, half) in self.terms.iter().zip(&halves) {
            for (sum, value) in lines[term.point].iter_mut().zip(half) {
                *sum += term.coefficient * value;
            }
        }

        // Both lines at 0, 1 and 2. eq of this coordinate z is 1 - z at 0
        // and z at 1, so 3z - 1 at 2.
        let mut evaluations = vec![F::zero(); 3];
        for ((&[at_zero, at_one], scalar), point) in
            lines.iter().zip(&self.scalars).zip(&self.points)
        {
            let z = point[round];
            evaluations[0] += *scalar * (F::one() - z) * at_zero;
            evaluations[1] += *scalar * z * at_one;
            evaluations[2] += *scalar * (z + z + z - F::one()) * (at_one + at_one - at_zero);
        }
        evaluations
    }
}

/// `table` with its lowest variable fixed to `r`, in the same allocation.
fn fold_in_place(table: &mut Vec<F>, r: F) {
    let half = table.len() / 2;
    for b in 0..half {
        table[b] = table[2 * b] + (table[2 * b + 1] - table[2 * b]) * r;
    }
    table.truncate(half);
}

/// A polynomial times a weight, with its lowest variables fixed to
/// challenges, computed entry by entry from its values.
struct Folded<'a> {
    fixed_vars: usize,
    weighted: Weighted<'a>,
}

enum Weighted<'a> {
    /// Field elements, and `weight * eq(c, challenges)` for every setting
    /// `c` of the fixed variables.
    Field(&'a [F], Vec<F>),
    /// Bytes, and the same times every byte value, at `c * 256 + byte`: an
    /// entry is then a sum of table entries.
    Bytes(&'a [u8], Vec<F>),
}

impl<'a> Folded<'a> {
    fn new(weight: F, values: Values<'a>, challenges: &[F]) -> Self {
        let eq = eq::table(challenges);
        let weights = eq.iter().map(|eq| *eq * weight);
        let weighted = match values {
            Values::Field(values) => Weighted::Field(values, weights.collect()),
            Values::Bytes(values) => {
                let multiples = weights.flat_map(|weight| {
                    std::iter::successors(Some(F::zero()), move |multiple| Some(*multiple + weight))
                        .take(1 << 8)
                });
                Weighted::Bytes(values, multiples.collect())
            }
        };
        Self {
            fixed_vars: challenges.len(),
            weighted,
        }
    }

    /// The entry at `index` of the folded polynomial.
    fn entry(&self, index: usize) -> F {
        let start = index << self.fixed_vars;
        let width = 1 << self.fixed_vars;
        match &self.weighted {
            Weighted::Field(values, weights) => row(values, start, width)
                .iter()
                .zip(weights)
                .map(|(value, weight)| *weight * value)
                .sum(),
            Weighted::Bytes(values, multiples) => row(values, start, width)
                .iter()
                .enumerate()
                .map(|(setting, &byte)| multiples[setting << 8 | usize::from(byte)])
                .sum(),
        }
    }

    /// `entry(2 * index + 1) - entry(2 * index)`, the quotient an opening
    /// commits to. Subtracting before weighting halves the multiplications.
    fn quotient(&self, index: usize) -> F {
        let width = 1 << self.fixed_vars;
        let start = (2 * index) << self.fixed_vars;
        match &self.weighted {
            Weighted::Field(values, weights) => {
                let odd = row(values, start + width, width);
                row(values, start, width)
                    .iter()
                    .zip(weights)
                    .enumerate()
                    .map(|(setting, (even, weight))| {
                        *weight * (odd.get(setting).copied().unwrap_or_default() - even)
                    })
                    .sum()
            }
            Weighted::Bytes(values, multiples) => {
                let odd = row(values, start + width, width);
                row(values, start, width)
                    .iter()
                    .enumerate()
                    .map(|(setting, &even)| {
                        let odd = odd.get(setting).copied().unwrap_or_default();
                        multiples[setting << 8 | usize::from(odd)]
                            - multiples[setting << 8 | usize::from(even)]
                    })
                    .sum()
            }
        }
    }

    /// The first `len` entries.
    fn table(&self, len: usize) -> Vec<F> {
        (0..len)
            .into_par_iter()
            .map(|index| self.entry(index))
            .collect()
    }
}

/// `values[start..start + width]`, or as much of it as is stored: the values
/// past the end are zero and add nothing.
fn row<T>(values: &[T], start: usize, width: usize) -> &[T] {
    let end = values.len().min(start + width);
    values.get(start..end).unwrap_or(&[])
}

/// The polynomial a batch reduces to: `sum of weights[i] * sources[i]`.
struct Combination<'a> {
    sources: &'a [&'a dyn Source],
    weights: Vec<F>,
    /// Entries that can be nonzero: no source has more values.
    live: usize,
}

impl Combination<'_> {
    /// `sum of of(weights[i] * sources[i], index)` for every index below
    /// `len`, where `of` reads a polynomial with its lowest variables fixed
    /// to `fixed`. One source is read at a time and added to the sums.
    fn sum(&self, fixed: &[F], len: usize, of: impl Fn(&Folded<'_>, usize) -> F + Sync) -> Vec<F> {
        let mut sums = vec![F::zero(); len];
        for (source, &weight) in self.sources.iter().zip(&self.weights) {
            source.read(&mut |values| {
                let folded = Folded::new(weight, values, fixed);
                sums.par_iter_mut()
                    .enumerate()
                    .for_each(|(index, sum)| *sum += of(&folded, index));
            });
        }
        sums
    }

    /// hyperplonk's opening of one polynomial at `point`, for this
    /// polynomial f. Round i commits to the quotient `f[2b + 1] - f[2b]` and
    /// fixes the lowest variable of f to `point[i]`.
    fn open(
        &self,
        srs: &Srs,
        num_vars: usize,
        point: &[F],
        lazy_quotients: usize,
    ) -> MultilinearKzgProof<Bls12_381> {
        let lazy = lazy_quotients.min(num_vars);
        // A quotient of zeros needs no work.
        let mut live = self.live;
        let mut proofs = Vec::with_capacity(num_vars);
        for round in 0..lazy {
            live = live.div_ceil(2);
            let quotient = self.sum(&point[..round], live, |folded, b| folded.quotient(b));
            let basis = srs.basis(num_vars - 1 - round);
            proofs.push(msm::msm(&basis, &quotient).into_affine());
        }
        if lazy < num_vars {
            let len = 1 << (num_vars - lazy);
            let mut table = self.sum(&point[..lazy], len, |folded, index| folded.entry(index));
            for (round, &coordinate) in point.iter().enumerate().skip(lazy) {
                live = live.div_ceil(2);
                let basis = srs.basis(num_vars - 1 - round);
                let quotient = msm::msm_with(&basis, live, |start, out| {
                    for (offset, quotient) in out.iter_mut().enumerate() {
                        let b = start + offset;
                        *quotient = table[2 * b + 1] - table[2 * b];
                    }
                });
                proofs.push(quotient.into_affine());
                fold_in_place(&mut table, coordinate);
            }
        }
        MultilinearKzgProof { proofs }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::Arc};

    use ark_ff::UniformRand;
    use ark_poly::{DenseMultilinearExtension, MultilinearExtension};
    use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
    use subroutines::pcs::prelude::{MultilinearProverParam, PolynomialCommitmentScheme};

    use super::*;
    use crate::{iop::Poly, Pcs};

    const MAX_VARS: usize = 10;

    enum Data {
        Bytes(Vec<u8>),
        Field(Vec<F>),
    }

    impl Data {
        fn values(&self) -> Values<'_> {
            match self {
                Data::Bytes(values) => Values::Bytes(values),
                Data::Field(values) => Values::Field(values),
            }
        }

        /// The polynomial as hyperplonk takes it: every value, padded.
        fn poly(&self, num_vars: usize) -> Poly {
            let values = self.values();
            let evaluations = (0..1 << num_vars).map(|x| values.get(x)).collect();
            Arc::new(DenseMultilinearExtension::from_evaluations_vec(
                num_vars,
                evaluations,
            ))
        }
    }

    /// A source that builds its values for each read, as the tables of a
    /// range check are: here they are a copy, which is gone when the read
    /// returns.
    struct Rebuilt<'a> {
        data: &'a Data,
        /// Set while any source of the batch is being read.
        reading: &'a Cell<bool>,
        reads: Cell<usize>,
    }

    impl Source for Rebuilt<'_> {
        fn read(&self, read: &mut dyn FnMut(Values<'_>)) {
            assert!(!self.reading.replace(true), "two sources read at once");
            let copy = match self.data {
                Data::Bytes(values) => Data::Bytes(values.clone()),
                Data::Field(values) => Data::Field(values.clone()),
            };
            read(copy.values());
            self.reading.set(false);
            self.reads.set(self.reads.get() + 1);
        }
    }

    /// A source that breaks the contract unless `times` is one: it passes
    /// its values that many times per read.
    struct Repeated<'a> {
        values: Values<'a>,
        times: usize,
    }

    impl Source for Repeated<'_> {
        fn read(&self, read: &mut dyn FnMut(Values<'_>)) {
            for _ in 0..self.times {
                read(self.values);
            }
        }
    }

    fn rebuilt<'a>(data: &'a [Data], reading: &'a Cell<bool>) -> Vec<Rebuilt<'a>> {
        data.iter()
            .map(|data| Rebuilt {
                data,
                reading,
                reads: Cell::new(0),
            })
            .collect()
    }

    fn as_sources<T: Source>(sources: &[T]) -> Vec<&dyn Source> {
        sources.iter().map(|source| source as &dyn Source).collect()
    }

    struct Instance {
        num_vars: usize,
        data: Vec<Data>,
        points: Vec<Vec<F>>,
        openings: Vec<Opening>,
    }

    /// Bytes and field elements, full and shorter than the hypercube; points
    /// that are random, boolean, or boolean at one end; openings that share
    /// points and polynomials; some evaluations fixed.
    fn instance(rng: &mut StdRng, num_vars: usize, count: usize) -> Instance {
        let size = 1usize << num_vars;
        let bytes =
            |rng: &mut StdRng, len: usize| Data::Bytes((0..len).map(|_| rng.gen()).collect());
        let field =
            |rng: &mut StdRng, len: usize| Data::Field((0..len).map(|_| F::rand(rng)).collect());
        let (short_bytes, short_field) = (rng.gen_range(0..size), rng.gen_range(1..size));
        let data = vec![
            bytes(rng, size),
            field(rng, size),
            bytes(rng, short_bytes),
            field(rng, short_field),
            bytes(rng, size - 1),
            field(rng, size),
        ];

        let random = |rng: &mut StdRng| (0..num_vars).map(|_| F::rand(rng)).collect::<Vec<F>>();
        let mut final_query = vec![F::one(); num_vars];
        final_query[0] = F::zero();
        let mut points = vec![vec![F::zero(); num_vars], final_query];
        for (coordinate, value) in [
            (0, F::zero()),
            (0, F::one()),
            (num_vars - 1, F::zero()),
            (num_vars - 1, F::one()),
        ] {
            let mut point = random(rng);
            point[coordinate] = value;
            points.push(point);
        }
        points.extend((0..3).map(|_| random(rng)));
        // Two points that differ in their first coordinate only.
        let mut twin = points[points.len() - 1].clone();
        twin[0] = F::one() - twin[0];
        points.push(twin);
        points.shuffle(rng);

        // Few points to choose from, so that small batches share them too.
        let choices = points.len().min(1 + count / 2);
        let openings = (0..count)
            .map(|_| {
                let commitment = rng.gen_range(0..data.len());
                let point = rng.gen_range(0..choices);
                let fixed = rng
                    .gen_bool(0.25)
                    .then(|| data[commitment].poly(num_vars).evaluate(&points[point]))
                    .flatten();
                Opening {
                    commitment,
                    point,
                    fixed,
                }
            })
            .collect();
        Instance {
            num_vars,
            data,
            points,
            openings,
        }
    }

    fn keys(rng: &mut StdRng, num_vars: usize) -> MultilinearProverParam<Bls12_381> {
        let srs = Pcs::gen_srs_for_testing(rng, num_vars).unwrap();
        Pcs::trim(&srs, None, Some(num_vars)).unwrap().0
    }

    fn transcript() -> IOPTranscript<F> {
        let mut transcript = IOPTranscript::new(b"batch opening test");
        transcript.append_message(b"seed", b"not empty").unwrap();
        transcript
    }

    /// hyperplonk's `multi_open` on the same batch, as `reference::open`
    /// calls it.
    fn hyperplonk(
        pcs: &MultilinearProverParam<Bls12_381>,
        instance: &Instance,
        transcript: &mut IOPTranscript<F>,
    ) -> BatchOpening {
        let mut selected = Vec::new();
        let mut at = Vec::new();
        let mut values = Vec::new();
        for opening in &instance.openings {
            let polynomial = instance.data[opening.commitment].poly(instance.num_vars);
            let point = &instance.points[opening.point];
            values.push(
                opening
                    .fixed
                    .unwrap_or_else(|| polynomial.evaluate(point).unwrap()),
            );
            selected.push(polynomial);
            at.push(point.clone());
        }
        Pcs::multi_open(pcs, &selected, &at, &values, transcript)
            .unwrap()
            .into()
    }

    /// The polynomials and points the prover opens, for a range check over
    /// `range_vars` variables, in the order of `iop::opening_points`: the
    /// image batch and the range batch index one list of each. The image is
    /// shorter than its hypercube.
    fn prover_shaped(rng: &mut StdRng, range_vars: usize) -> (Vec<Data>, Vec<Vec<F>>) {
        let image_vars = range_vars - 1;
        let mut data: Vec<Data> = (0..3)
            .map(|_| Data::Bytes((0..(1 << image_vars) - 37).map(|_| rng.gen()).collect()))
            .collect();
        for _ in 0..3 {
            data.push(Data::Bytes(
                (0..1 << range_vars).map(|_| rng.gen()).collect(),
            ));
            for _ in 0..2 {
                data.push(Data::Field(
                    (0..1 << range_vars).map(|_| F::rand(rng)).collect(),
                ));
            }
        }

        let random = |rng: &mut StdRng, len: usize| (0..len).map(|_| F::rand(rng)).collect();
        let mut points: Vec<Vec<F>> = (0..3).map(|_| random(rng, image_vars)).collect();
        points.push(vec![F::zero(); range_vars]);
        let mut final_query = vec![F::one(); range_vars];
        final_query[0] = F::zero();
        points.push(final_query);
        for _ in 0..3 {
            let r: Vec<F> = random(rng, range_vars);
            let without_last = &r[..image_vars];
            let mut fiddled: Vec<F> = r[1..]
                .iter()
                .enumerate()
                .map(|(bit, &coordinate)| {
                    if bit % 3 == 0 {
                        F::one() - coordinate
                    } else {
                        coordinate
                    }
                })
                .collect();
            fiddled.push(F::one());
            let mut shifted = r[1..].to_vec();
            shifted.push(F::zero());
            points.extend([
                without_last.to_vec(),
                fiddled,
                shifted,
                r.clone(),
                [&[F::zero()], without_last].concat(),
                [&[F::one()], without_last].concat(),
            ]);
        }
        points.extend((0..3).map(|_| random(rng, image_vars)));
        (data, points)
    }

    fn assert_same(ours: &BatchOpening, theirs: &BatchOpening, case: &str) {
        assert_eq!(ours.sumcheck.point, theirs.sumcheck.point, "{case}");
        assert_eq!(ours.sumcheck.rounds, theirs.sumcheck.rounds, "{case}");
        assert_eq!(ours.evaluations, theirs.evaluations, "{case}");
        assert_eq!(ours.kzg, theirs.kzg, "{case}");
    }

    fn assert_same_state(mut ours: IOPTranscript<F>, mut theirs: IOPTranscript<F>, case: &str) {
        assert_eq!(
            ours.get_and_append_challenge(b"next").unwrap(),
            theirs.get_and_append_challenge(b"next").unwrap(),
            "{case}"
        );
    }

    /// Opens `instance` here and with hyperplonk, each on a fresh transcript,
    /// and asserts that the proofs and the transcripts agree. Here it is
    /// opened twice: from values held in memory, and from sources that build
    /// theirs for each read. Returns ours.
    fn assert_opens_as_hyperplonk(
        pcs: &MultilinearProverParam<Bls12_381>,
        instance: &Instance,
        case: &str,
    ) -> BatchOpening {
        let srs = Srs::Loaded(pcs.clone());
        let mut theirs_transcript = transcript();
        let theirs = hyperplonk(pcs, instance, &mut theirs_transcript);
        let open = |polynomials: &[&dyn Source], case: &str| {
            let mut ours_transcript = transcript();
            let ours = open(
                &srs,
                instance.num_vars,
                polynomials,
                &instance.points,
                &instance.openings,
                &mut ours_transcript,
            )
            .unwrap();
            assert_same(&ours, &theirs, case);
            assert_same_state(ours_transcript, theirs_transcript.clone(), case);
            ours
        };
        let held: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
        let ours = open(&as_sources(&held), case);
        let reading = Cell::new(false);
        let built = rebuilt(&instance.data, &reading);
        open(&as_sources(&built), &format!("rebuilt, {case}"));
        ours
    }

    #[test]
    fn opens_as_hyperplonk_does() {
        let mut rng = StdRng::seed_from_u64(7);
        // Larger than most of the polynomials, as the image batch finds it.
        let pcs = keys(&mut rng, MAX_VARS);
        for num_vars in 1..=MAX_VARS {
            for count in [2, 3, 9, 33] {
                let case = format!("{count} openings over {num_vars} variables");
                let instance = instance(&mut rng, num_vars, count);
                assert_opens_as_hyperplonk(&pcs, &instance, &case);
            }
        }
    }

    /// When every polynomial of a batch is short, the last entries of the
    /// combination are zero and their quotients are not asked for. When
    /// every polynomial is empty, no quotient is.
    #[test]
    fn opens_batches_of_short_polynomials() {
        let mut rng = StdRng::seed_from_u64(12);
        let pcs = keys(&mut rng, MAX_VARS - 1);
        for num_vars in [1, 2, 3, 6, MAX_VARS - 1] {
            let size = 1usize << num_vars;
            let random = |rng: &mut StdRng| (0..num_vars).map(|_| F::rand(rng)).collect::<Vec<F>>();
            let mut final_query = vec![F::one(); num_vars];
            final_query[0] = F::zero();
            let points = vec![
                random(&mut rng),
                random(&mut rng),
                vec![F::zero(); num_vars],
                final_query,
            ];
            let over_half = (size / 2 + 1).min(size - 1);
            for (bytes, field) in [
                (0, 0),
                (1, 0),
                (0, 1),
                (1, 1),
                (size / 2, size / 2),
                (over_half, 1),
                (1, over_half),
                (size / 4 + 1, size / 8),
                (size - 1, size - 1),
            ] {
                for (kind, commitments) in [("both", 0..4), ("bytes", 0..2), ("field", 2..4)] {
                    // Polynomials 0 and 1 are of bytes, 2 and 3 of field
                    // elements.
                    let data = vec![
                        Data::Bytes((0..bytes).map(|_| rng.gen()).collect()),
                        Data::Bytes((0..bytes / 2).map(|_| rng.gen()).collect()),
                        Data::Field((0..field).map(|_| F::rand(&mut rng)).collect()),
                        Data::Field((0..field / 2).map(|_| F::rand(&mut rng)).collect()),
                    ];
                    let openings = (0..5)
                        .map(|i| Opening {
                            commitment: commitments.start + i % commitments.len(),
                            point: rng.gen_range(0..points.len()),
                            fixed: None,
                        })
                        .collect();
                    let instance = Instance {
                        num_vars,
                        data,
                        points: points.clone(),
                        openings,
                    };
                    let case = format!(
                        "{kind}: {bytes} bytes and {field} field elements over {num_vars} variables"
                    );
                    assert_opens_as_hyperplonk(&pcs, &instance, &case);
                }
            }
        }
    }

    /// The image and the range batch fix more than `BYTE_BLOCK_VARS` high
    /// variables of their polynomials of bytes, so a pass takes several
    /// blocks and reuses its sums per byte value from one block to the
    /// next. The batches above are too small for a second block. A low table
    /// of fewer variables reaches it on small data.
    #[test]
    fn fixes_the_high_variables_of_bytes_block_by_block() {
        let mut rng = StdRng::seed_from_u64(13);
        for num_vars in 1..=9 {
            let size = 1usize << num_vars;
            for low_vars in 0..=num_vars.min(3) {
                let width = 1 << low_vars;
                let random: Vec<F> = (low_vars..num_vars).map(|_| F::rand(&mut rng)).collect();
                let mut ends_in_zero = random.clone();
                if let Some(last) = ends_in_zero.last_mut() {
                    *last = F::zero();
                }
                let tails = [
                    vec![F::zero(); random.len()],
                    vec![F::one(); random.len()],
                    ends_in_zero,
                    random,
                ];
                for (tail, len) in tails.iter().flat_map(|tail| {
                    [0, 1, size / 3, size / 2 + 1, size - 1, size].map(|len| (tail, len))
                }) {
                    let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                    let field: Vec<F> = bytes.iter().map(|&byte| F::from(byte)).collect();
                    let mut expected = vec![F::zero(); width];
                    for (weight, row) in eq::table(tail).iter().zip(field.chunks(width)) {
                        for (sum, value) in expected.iter_mut().zip(row) {
                            *sum += *weight * value;
                        }
                    }
                    // One more than the tail has: the split takes what there is.
                    for block_vars in 0..=tail.len() + 1 {
                        let case = format!(
                            "{len} of 2^{num_vars} values, {low_vars} low variables, \
                             blocks of {block_vars} variables"
                        );
                        let split = SplitEq::with_low_vars(tail, block_vars);
                        assert_eq!(
                            fix_high(Values::Bytes(&bytes), low_vars, &split),
                            expected,
                            "bytes: {case}"
                        );
                        assert_eq!(
                            fix_high(Values::Field(&field), low_vars, &split),
                            expected,
                            "field elements: {case}"
                        );
                    }
                    assert_eq!(head(Values::Bytes(&bytes), low_vars, tail), expected);
                    assert_eq!(head(Values::Field(&field), low_vars, tail), expected);
                }
            }
        }
    }

    #[test]
    fn opens_the_batches_of_the_prover() {
        let mut rng = StdRng::seed_from_u64(11);
        let range_vars = 9;
        let pcs = keys(&mut rng, range_vars);
        let srs = Srs::Loaded(pcs.clone());
        let (data, points) = prover_shaped(&mut rng, range_vars);
        let mut ours_transcript = transcript();
        let mut theirs_transcript = transcript();
        // Both batches on one transcript, the image batch first.
        let mut instance = Instance {
            num_vars: range_vars - 1,
            data,
            points,
            openings: crate::iop::image_openings(),
        };
        for case in ["image batch", "range batch"] {
            // As the prover holds them: the image and the counts in memory,
            // the products and the fractions built for each read.
            let held: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
            let reading = Cell::new(false);
            let built = rebuilt(&instance.data, &reading);
            let polynomials: Vec<&dyn Source> = (0..held.len())
                .map(|index| match instance.data[index] {
                    Data::Bytes(_) => &held[index] as &dyn Source,
                    Data::Field(_) => &built[index],
                })
                .collect();
            let theirs = hyperplonk(&pcs, &instance, &mut theirs_transcript);
            let ours = open(
                &srs,
                instance.num_vars,
                &polynomials,
                &instance.points,
                &instance.openings,
                &mut ours_transcript,
            )
            .unwrap();
            assert_same(&ours, &theirs, case);
            instance.num_vars = range_vars;
            instance.openings = crate::iop::range_openings();
        }
        assert_same_state(ours_transcript, theirs_transcript, "both batches");
    }

    #[test]
    fn lazy_rounds_do_not_change_the_proof() {
        let mut rng = StdRng::seed_from_u64(8);
        let num_vars = 7;
        let pcs = keys(&mut rng, num_vars);
        let srs = Srs::Loaded(pcs.clone());
        let instance = instance(&mut rng, num_vars, 9);
        let mut theirs_transcript = transcript();
        let theirs = hyperplonk(&pcs, &instance, &mut theirs_transcript);
        for (lazy_rounds, lazy_quotients) in [(1, 0), (2, 1), (3, 6), (6, 7), (7, 2), (9, 9)] {
            let case = format!("{lazy_rounds} lazy rounds, {lazy_quotients} lazy quotients");
            let reading = Cell::new(false);
            let built = rebuilt(&instance.data, &reading);
            let mut ours_transcript = transcript();
            let ours = open_with(
                &srs,
                num_vars,
                &as_sources(&built),
                &instance.points,
                &instance.openings,
                &mut ours_transcript,
                lazy_rounds,
                lazy_quotients,
            )
            .unwrap();
            assert_same(&ours, &theirs, &case);
            assert_same_state(ours_transcript, theirs_transcript.clone(), &case);
        }
    }

    /// What a source that builds its values costs: one read for the
    /// evaluations and the lazy rounds, one for the fold, one per lazy
    /// quotient and one for the table of the combination. A polynomial the
    /// batch does not open is never read.
    #[test]
    fn reads_a_source_once_per_step() {
        let mut rng = StdRng::seed_from_u64(14);
        let num_vars = 6;
        let srs = Srs::Loaded(keys(&mut rng, num_vars));
        let mut instance = instance(&mut rng, num_vars, 9);
        // Polynomials 0 to 3 are opened, 4 and 5 are not.
        for (index, opening) in instance.openings.iter_mut().enumerate() {
            opening.commitment = index % 4;
            opening.fixed = None;
        }
        for (lazy_rounds, lazy_quotients, expected) in [
            (2, 0, 1 + 1 + 1),
            (2, 3, 1 + 1 + 3 + 1),
            // No fold when the lazy rounds are all the rounds, and no table
            // when the lazy quotients are all the quotients.
            (6, 3, 1 + 3 + 1),
            (2, 6, 1 + 1 + 6),
        ] {
            let reading = Cell::new(false);
            let built = rebuilt(&instance.data, &reading);
            open_with(
                &srs,
                num_vars,
                &as_sources(&built),
                &instance.points,
                &instance.openings,
                &mut transcript(),
                lazy_rounds,
                lazy_quotients,
            )
            .unwrap();
            let reads: Vec<usize> = built.iter().map(|source| source.reads.get()).collect();
            assert_eq!(
                reads,
                [expected, expected, expected, expected, 0, 0],
                "{lazy_rounds} lazy rounds, {lazy_quotients} lazy quotients"
            );
        }
    }

    #[test]
    fn a_fixed_evaluation_is_written_as_given() {
        let mut rng = StdRng::seed_from_u64(9);
        let num_vars = 5;
        let pcs = keys(&mut rng, num_vars);
        let mut instance = instance(&mut rng, num_vars, 3);
        let fixed = F::rand(&mut rng);
        instance.openings[1].fixed = Some(fixed);
        let ours = assert_opens_as_hyperplonk(&pcs, &instance, "a fixed evaluation");
        assert_eq!(ours.evaluations[1], fixed);
    }

    #[test]
    fn refuses_what_it_cannot_open() {
        let mut rng = StdRng::seed_from_u64(10);
        let num_vars = 4;
        let srs = Srs::Loaded(keys(&mut rng, num_vars));
        let instance = instance(&mut rng, num_vars, 3);
        let polynomials: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
        let attempt = |num_vars, polynomials: &[Values<'_>], points: &[Vec<F>], openings| {
            open(
                &srs,
                num_vars,
                &as_sources(polynomials),
                points,
                openings,
                &mut transcript(),
            )
        };
        assert!(attempt(num_vars, &polynomials, &instance.points, &instance.openings).is_ok());
        assert!(attempt(
            num_vars,
            &polynomials,
            &instance.points,
            &instance.openings[..1]
        )
        .is_err());
        assert!(attempt(
            num_vars + 1,
            &polynomials,
            &instance.points,
            &instance.openings
        )
        .is_err());
        assert!(attempt(num_vars, &[], &instance.points, &instance.openings).is_err());
        assert!(attempt(num_vars, &polynomials, &[], &instance.openings).is_err());
        let longer: Vec<Vec<F>> = instance
            .points
            .iter()
            .map(|point| [point.as_slice(), &[F::one()]].concat())
            .collect();
        assert!(attempt(num_vars, &polynomials, &longer, &instance.openings).is_err());
        let too_long = vec![0u8; (1 << num_vars) + 1];
        let oversized = vec![Values::Bytes(&too_long); polynomials.len()];
        assert!(attempt(num_vars, &oversized, &instance.points, &instance.openings).is_err());
    }

    /// A source that keeps its values to itself would be opened as the zero
    /// polynomial, and one that passes them twice would be read twice. Both
    /// are refused before anything is written to the transcript.
    #[test]
    fn refuses_a_source_that_does_not_pass_its_values_once() {
        let mut rng = StdRng::seed_from_u64(15);
        let num_vars = 4;
        let srs = Srs::Loaded(keys(&mut rng, num_vars));
        let instance = instance(&mut rng, num_vars, 3);
        let untouched = transcript().get_and_append_challenge(b"next").unwrap();
        for times in 0..=2 {
            // Only the first polynomial an opening names misbehaves.
            let polynomials: Vec<Repeated<'_>> = (0..instance.data.len())
                .map(|index| Repeated {
                    values: instance.data[index].values(),
                    times: if index == instance.openings[0].commitment {
                        times
                    } else {
                        1
                    },
                })
                .collect();
            let mut transcript = transcript();
            let opened = open(
                &srs,
                num_vars,
                &as_sources(&polynomials),
                &instance.points,
                &instance.openings,
                &mut transcript,
            );
            assert_eq!(opened.is_ok(), times == 1, "{times} times");
            if times != 1 {
                assert_eq!(
                    transcript.get_and_append_challenge(b"next").unwrap(),
                    untouched,
                    "{times} times"
                );
            }
        }
    }
}
