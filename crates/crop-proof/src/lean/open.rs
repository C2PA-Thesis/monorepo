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
//!   time. The first quotients are computed from the polynomials, entry by
//!   entry, as the commitment asks for them. Only then is the combination
//!   stored, as a small table.

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
/// polynomials. The table of the combination stored afterwards is
/// `2^LAZY_QUOTIENTS` times smaller than a polynomial. Each round costs one
/// multiplication per two values of every polynomial of field elements. On
/// the range batch over 18 variables, three rounds and building the table
/// measured about 4% of the time of committing to the quotients.
const LAZY_QUOTIENTS: usize = 3;

/// Variables of the low eq table when the values are bytes. Sums are kept
/// per byte value and settled once per block, so a block must hold many more
/// values than there are byte values.
const BYTE_BLOCK_VARS: usize = 12;

/// Opens `polynomials[opening.commitment]` at `points[opening.point]` for
/// every opening, with the transcript and the proof of hyperplonk's
/// `multi_open`. Every polynomial an opening names has `num_vars` variables.
pub(crate) fn open(
    srs: &Srs,
    num_vars: usize,
    polynomials: &[Values<'_>],
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
    polynomials: &[Values<'_>],
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
        let values = polynomials
            .get(opening.commitment)
            .with_context(|| format!("no polynomial {} to open", opening.commitment))?;
        ensure!(
            values.len() <= 1 << num_vars,
            "polynomial {} has more than 2^{num_vars} values",
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

    // One pass over a polynomial serves the evaluation and the lazy rounds
    // of every opening of it at points that agree in their high coordinates.
    let lazy_vars = lazy_rounds.clamp(1, num_vars);
    let (passes, pass_slots) = distinct(
        openings
            .iter()
            .map(|opening| (opening.commitment, &points[opening.point][lazy_vars..])),
    );
    let pass_heads: Vec<Vec<F>> = passes
        .par_iter()
        .map(|&(commitment, tail)| head(polynomials[commitment], lazy_vars, tail))
        .collect();
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

    let (source_ids, source_slots) = distinct(openings.iter().map(|opening| opening.commitment));
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
        source_ids.iter().map(|&id| polynomials[id]).collect(),
        point_ids.iter().map(|&id| points[id].as_slice()).collect(),
        terms,
        lazy_vars,
        &heads,
    );
    let sumcheck = sumcheck::prove(&mut batch, transcript)?;

    let combination = batch.into_combination(&sumcheck.point);
    let kzg = open_combination(srs, num_vars, &combination, &sumcheck.point, lazy_quotients);
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
    sources: Vec<Values<'a>>,
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
        sources: Vec<Values<'a>>,
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

    /// The polynomial the batch reduces to at the sumcheck's point `a`: each
    /// source with the sum of `eq(a, point) * coefficient` over its openings.
    /// Gives up the folded tables.
    fn into_combination(self, a: &[F]) -> Vec<(F, Values<'a>)> {
        let at_points: Vec<F> = self.points.iter().map(|point| eq::eval(a, point)).collect();
        let mut weights = vec![F::zero(); self.sources.len()];
        for term in &self.terms {
            weights[term.source] += term.coefficient * at_points[term.point];
        }
        weights.into_iter().zip(self.sources).collect()
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
            self.tables = self
                .sources
                .iter()
                .map(|&source| Folded::new([(F::one(), source)], &self.challenges).table(len))
                .collect();
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

/// A weighted sum of polynomials with their lowest variables fixed to
/// challenges, computed entry by entry from the values of the polynomials.
struct Folded<'a> {
    fixed_vars: usize,
    /// Per polynomial of field elements, `weight * eq(c, challenges)` for
    /// every setting `c` of the fixed variables.
    fields: Vec<(&'a [F], Vec<F>)>,
    /// Per polynomial of bytes, the same times every byte value, at
    /// `c * 256 + byte`: an entry is then a sum of table entries.
    bytes: Vec<(&'a [u8], Vec<F>)>,
}

impl<'a> Folded<'a> {
    fn new(terms: impl IntoIterator<Item = (F, Values<'a>)>, challenges: &[F]) -> Self {
        let eq = eq::table(challenges);
        let mut fields = Vec::new();
        let mut bytes = Vec::new();
        for (weight, values) in terms {
            let weights = eq.iter().map(|eq| *eq * weight);
            match values {
                Values::Field(values) => fields.push((values, weights.collect())),
                Values::Bytes(values) => {
                    let multiples = weights.flat_map(|weight| {
                        std::iter::successors(Some(F::zero()), move |multiple| {
                            Some(*multiple + weight)
                        })
                        .take(1 << 8)
                    });
                    bytes.push((values, multiples.collect()));
                }
            }
        }
        Self {
            fixed_vars: challenges.len(),
            fields,
            bytes,
        }
    }

    /// The entry at `index` of the folded sum.
    fn entry(&self, index: usize) -> F {
        let start = index << self.fixed_vars;
        let width = 1 << self.fixed_vars;
        let mut sum = F::zero();
        for (values, weights) in &self.fields {
            for (value, weight) in row(values, start, width).iter().zip(weights) {
                sum += *weight * value;
            }
        }
        for (values, multiples) in &self.bytes {
            for (setting, &byte) in row(values, start, width).iter().enumerate() {
                sum += multiples[setting << 8 | usize::from(byte)];
            }
        }
        sum
    }

    /// `entry(2 * index + 1) - entry(2 * index)`, the quotient an opening
    /// commits to. Subtracting before weighting halves the multiplications.
    fn quotient(&self, index: usize) -> F {
        let width = 1 << self.fixed_vars;
        let start = (2 * index) << self.fixed_vars;
        let mut sum = F::zero();
        for (values, weights) in &self.fields {
            let odd = row(values, start + width, width);
            for (setting, (even, weight)) in
                row(values, start, width).iter().zip(weights).enumerate()
            {
                sum += *weight * (odd.get(setting).copied().unwrap_or_default() - even);
            }
        }
        for (values, multiples) in &self.bytes {
            let odd = row(values, start + width, width);
            for (setting, &even) in row(values, start, width).iter().enumerate() {
                let odd = odd.get(setting).copied().unwrap_or_default();
                sum += multiples[setting << 8 | usize::from(odd)];
                sum -= multiples[setting << 8 | usize::from(even)];
            }
        }
        sum
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

/// hyperplonk's opening of one polynomial at `point`, for the polynomial
/// `sum of weight * values` over `terms`. Round i commits to the quotient
/// `f[2b + 1] - f[2b]` and fixes the lowest variable of f to `point[i]`.
fn open_combination(
    srs: &Srs,
    num_vars: usize,
    terms: &[(F, Values<'_>)],
    point: &[F],
    lazy_quotients: usize,
) -> MultilinearKzgProof<Bls12_381> {
    let lazy = lazy_quotients.min(num_vars);
    // Entries of f that can be nonzero. Short polynomials end in zeros, and
    // a quotient of zeros needs no work.
    let mut live = terms
        .iter()
        .map(|(_, values)| values.len())
        .max()
        .unwrap_or(0);
    let mut proofs = Vec::with_capacity(num_vars);
    for round in 0..lazy {
        let folded = Folded::new(terms.iter().copied(), &point[..round]);
        live = live.div_ceil(2);
        let quotient = msm::msm_with(&srs.basis(num_vars - 1 - round), live, |start, out| {
            for (offset, quotient) in out.iter_mut().enumerate() {
                *quotient = folded.quotient(start + offset);
            }
        });
        proofs.push(quotient.into_affine());
    }
    if lazy < num_vars {
        let mut table =
            Folded::new(terms.iter().copied(), &point[..lazy]).table(1 << (num_vars - lazy));
        for (round, &coordinate) in point.iter().enumerate().skip(lazy) {
            live = live.div_ceil(2);
            let quotient = msm::msm_with(&srs.basis(num_vars - 1 - round), live, |start, out| {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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

    #[test]
    fn opens_as_hyperplonk_does() {
        let mut rng = StdRng::seed_from_u64(7);
        // Larger than most of the polynomials, as the image batch finds it.
        let pcs = keys(&mut rng, MAX_VARS);
        let srs = Srs::Loaded(pcs.clone());
        for num_vars in 3..=MAX_VARS {
            for count in [2, 3, 9, 33] {
                let case = format!("{count} openings over {num_vars} variables");
                let instance = instance(&mut rng, num_vars, count);
                let polynomials: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
                let mut theirs_transcript = transcript();
                let theirs = hyperplonk(&pcs, &instance, &mut theirs_transcript);
                let mut ours_transcript = transcript();
                let ours = open(
                    &srs,
                    num_vars,
                    &polynomials,
                    &instance.points,
                    &instance.openings,
                    &mut ours_transcript,
                )
                .unwrap();
                assert_same(&ours, &theirs, &case);
                assert_same_state(ours_transcript, theirs_transcript, &case);
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
            let polynomials: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
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
        let polynomials: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
        let mut theirs_transcript = transcript();
        let theirs = hyperplonk(&pcs, &instance, &mut theirs_transcript);
        for (lazy_rounds, lazy_quotients) in [(1, 0), (2, 1), (3, 6), (6, 7), (7, 2), (9, 9)] {
            let case = format!("{lazy_rounds} lazy rounds, {lazy_quotients} lazy quotients");
            let mut ours_transcript = transcript();
            let ours = open_with(
                &srs,
                num_vars,
                &polynomials,
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

    #[test]
    fn a_fixed_evaluation_is_written_as_given() {
        let mut rng = StdRng::seed_from_u64(9);
        let num_vars = 5;
        let pcs = keys(&mut rng, num_vars);
        let mut instance = instance(&mut rng, num_vars, 3);
        let fixed = F::rand(&mut rng);
        instance.openings[1].fixed = Some(fixed);
        let polynomials: Vec<Values<'_>> = instance.data.iter().map(Data::values).collect();
        let mut theirs_transcript = transcript();
        let theirs = hyperplonk(&pcs, &instance, &mut theirs_transcript);
        let mut ours_transcript = transcript();
        let ours = open(
            &Srs::Loaded(pcs.clone()),
            num_vars,
            &polynomials,
            &instance.points,
            &instance.openings,
            &mut ours_transcript,
        )
        .unwrap();
        assert_eq!(ours.evaluations[1], fixed);
        assert_same(&ours, &theirs, "a fixed evaluation");
        assert_same_state(ours_transcript, theirs_transcript, "a fixed evaluation");
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
                polynomials,
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
}
