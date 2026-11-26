//! eq(x, point) without its full table: the table is the outer product of
//! two tables of about the square root of its size.

use ark_ff::{One, Zero};
use rayon::prelude::*;

use crate::F;

/// `eq(x, point)` for every x of the hypercube, with bit i of the index as
/// the value of variable i, as hyperplonk's `build_eq_x_r_vec` lays it out.
pub(super) fn table(point: &[F]) -> Vec<F> {
    let mut table = Vec::with_capacity(1 << point.len());
    table.push(F::one());
    for &coordinate in point {
        let half = table.len();
        for i in 0..half {
            let high = table[i] * coordinate;
            table.push(high);
            table[i] -= high;
        }
    }
    table
}

/// Values a task of `SplitEq::sum_rows` reads at least, unless there are
/// fewer: below this, starting the task costs more than its work.
const TASK_VALUES: usize = 1 << 12;

/// `eq(x, point)` as `low[x mod 2^k] * high[x div 2^k]`.
pub(super) struct SplitEq {
    low: Vec<F>,
    high: Vec<F>,
    low_vars: usize,
}

impl SplitEq {
    /// Two tables of about the same size, which keeps both small.
    pub(super) fn new(point: &[F]) -> Self {
        Self::with_low_vars(point, point.len() / 2)
    }

    /// As `new`, with the first `low_vars` coordinates in the low table. A
    /// caller that keeps state per entry of the high table picks the split.
    pub(super) fn with_low_vars(point: &[F], low_vars: usize) -> Self {
        let low_vars = low_vars.min(point.len());
        Self {
            low: table(&point[..low_vars]),
            high: table(&point[low_vars..]),
            low_vars,
        }
    }

    /// `eq(x, point)`.
    pub(super) fn at(&self, x: usize) -> F {
        self.low[x & (self.low.len() - 1)] * self.high[x >> self.low_vars]
    }

    /// `sum over x of eq(x, point) * terms(x)`, one sum per term, where
    /// `terms` is cheap to call in any order.
    pub(super) fn sum<const N: usize>(&self, terms: impl Fn(usize) -> [F; N] + Sync) -> [F; N] {
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

    /// `sum over x of eq(x, point) * row(x)`, where a row is `width` values.
    /// The x that share an entry of the high table form a block:
    /// `block(scratch, first, low, sums)` adds `low[i] * row(first + i)` for
    /// every `i` to `sums`, which it receives as zeros. Blocks whose high
    /// entry is zero are skipped, so a point that ends in zeros and ones
    /// costs its share of the hypercube only. Each task gets its own
    /// `scratch`, and enough blocks to be worth starting.
    pub(super) fn sum_rows<S: Send>(
        &self,
        width: usize,
        scratch: impl Fn() -> S + Sync,
        block: impl Fn(&mut S, usize, &[F], &mut [F]) + Sync,
    ) -> Vec<F> {
        let blocks = TASK_VALUES.div_ceil(self.low.len() * width);
        self.high
            .par_iter()
            .enumerate()
            .with_min_len(blocks)
            .fold(
                || (scratch(), vec![F::zero(); width], vec![F::zero(); width]),
                |(mut scratch, mut sums, mut total), (high_index, high)| {
                    if !high.is_zero() {
                        sums.fill(F::zero());
                        let first = high_index << self.low_vars;
                        block(&mut scratch, first, &self.low, &mut sums);
                        for (total, sum) in total.iter_mut().zip(&sums) {
                            *total += *high * sum;
                        }
                    }
                    (scratch, sums, total)
                },
            )
            .map(|(_, _, total)| total)
            .reduce(
                || vec![F::zero(); width],
                |mut left, right| {
                    for (sum, value) in left.iter_mut().zip(right) {
                        *sum += value;
                    }
                    left
                },
            )
    }
}

/// `eq(x, y)` for two points of the same length.
pub(super) fn eval(x: &[F], y: &[F]) -> F {
    x.iter().zip(y).map(|(x, y)| line(*x, *y)).product()
}

/// `eq` of one coordinate: `x * y + (1 - x) * (1 - y)`.
pub(super) fn line(x: F, y: F) -> F {
    let product = x * y;
    product + product - x - y + F::one()
}

#[cfg(test)]
mod tests {
    use arithmetic::{build_eq_x_r_vec, eq_eval};
    use ark_ff::UniformRand;
    use rand::SeedableRng;

    use super::*;

    #[test]
    fn tables_match_hyperplonk() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for num_vars in 1..9 {
            let point: Vec<F> = (0..num_vars).map(|_| F::rand(&mut rng)).collect();
            let expected = build_eq_x_r_vec(&point).unwrap();
            assert_eq!(table(&point), expected);
            // The zero check of the range check sums several terms at once
            // and reads single entries.
            let split = SplitEq::new(&point);
            for (x, eq) in expected.iter().enumerate() {
                assert_eq!(split.at(x), *eq);
            }
            let terms: Vec<[F; 3]> = expected
                .iter()
                .map(|_| std::array::from_fn(|_| F::rand(&mut rng)))
                .collect();
            let mut direct = [F::zero(); 3];
            for (eq, terms) in expected.iter().zip(&terms) {
                for (sum, term) in direct.iter_mut().zip(terms) {
                    *sum += *eq * term;
                }
            }
            assert_eq!(split.sum(|x| terms[x]), direct);
            assert_eq!(split.sum(|x| [terms[x][1]]), [direct[1]]);
        }
        // hyperplonk has no table for no variables. The last round of a
        // sumcheck asks for it: eq is one.
        let empty = SplitEq::new(&[]);
        assert_eq!(empty.at(0), F::one());
        assert_eq!(empty.sum(|x| [F::from(x as u64 + 7)]), [F::from(7u64)]);
    }

    #[test]
    fn rows_sum_as_over_the_full_table() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let width = 4;
        for num_vars in 0..8 {
            let mut point: Vec<F> = (0..num_vars).map(|_| F::rand(&mut rng)).collect();
            // Zeros and ones at the end leave blocks to skip.
            for (coordinate, value) in point.iter_mut().rev().zip([F::one(), F::zero()]) {
                *coordinate = value;
            }
            let other: Vec<F> = (0..num_vars).map(|_| F::rand(&mut rng)).collect();
            assert_eq!(eval(&point, &other), eq_eval(&point, &other).unwrap());

            let values: Vec<F> = (0..width << num_vars).map(|_| F::rand(&mut rng)).collect();
            let mut expected = vec![F::zero(); width];
            for (eq, row) in table(&point).iter().zip(values.chunks(width)) {
                for (sum, value) in expected.iter_mut().zip(row) {
                    *sum += *eq * value;
                }
            }
            for low_vars in 0..=num_vars + 1 {
                let split = SplitEq::with_low_vars(&point, low_vars);
                let sums = split.sum_rows(
                    width,
                    || (),
                    |(), first, low, sums| {
                        let rows = values[first * width..].chunks(width);
                        for (row, weight) in rows.zip(low) {
                            for (sum, value) in sums.iter_mut().zip(row) {
                                *sum += *weight * value;
                            }
                        }
                    },
                );
                assert_eq!(sums, expected, "{num_vars} variables, {low_vars} low");
            }
        }
    }
}
