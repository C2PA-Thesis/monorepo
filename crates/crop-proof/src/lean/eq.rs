//! eq(x, point) without its full table: the table is the outer product of
//! two tables of about the square root of its size.

use ark_ff::{One, Zero};
use rayon::prelude::*;

use crate::F;

/// `eq(x, point)` for every x of the hypercube, with bit i of the index as
/// the value of variable i, as hyperplonk's `build_eq_x_r_vec` lays it out.
pub(crate) fn table(point: &[F]) -> Vec<F> {
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

/// `eq(x, point)` as `low[x mod 2^k] * high[x div 2^k]`.
pub(crate) struct SplitEq {
    low: Vec<F>,
    high: Vec<F>,
    low_vars: usize,
}

impl SplitEq {
    pub(crate) fn new(point: &[F]) -> Self {
        let low_vars = point.len() / 2;
        Self {
            low: table(&point[..low_vars]),
            high: table(&point[low_vars..]),
            low_vars,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.low.len() * self.high.len()
    }

    /// `sum over x of eq(x, point) * value(x)`, where `value` is cheap to
    /// call in any order.
    #[cfg(test)]
    pub(crate) fn sum(&self, value: impl Fn(usize) -> F + Sync) -> F {
        self.high
            .par_iter()
            .enumerate()
            .map(|(high_index, high)| {
                let base = high_index << self.low_vars;
                let inner = self
                    .low
                    .iter()
                    .enumerate()
                    .fold(F::zero(), |sum, (low_index, low)| {
                        sum + *low * value(base | low_index)
                    });
                inner * high
            })
            .sum()
    }
}

/// Values a task of `SplitEq::sum_rows` reads at least, unless there are
/// fewer: below this, starting the task costs more than its work.
const TASK_VALUES: usize = 1 << 12;

impl SplitEq {
    /// As `new`, with the first `low_vars` coordinates in the low table. A
    /// caller that keeps state per entry of the high table picks the split.
    pub(crate) fn with_low_vars(point: &[F], low_vars: usize) -> Self {
        let low_vars = low_vars.min(point.len());
        Self {
            low: table(&point[..low_vars]),
            high: table(&point[low_vars..]),
            low_vars,
        }
    }

    /// `sum over x of eq(x, point) * row(x)`, where a row is `width` values.
    /// The x that share an entry of the high table form a block:
    /// `block(scratch, first, low, sums)` adds `low[i] * row(first + i)` for
    /// every `i` to `sums`, which it receives as zeros. Blocks whose high
    /// entry is zero are skipped, so a point that ends in zeros and ones
    /// costs its share of the hypercube only. Each task gets its own
    /// `scratch`, and enough blocks to be worth starting.
    pub(crate) fn sum_rows<S: Send>(
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
pub(crate) fn eval(x: &[F], y: &[F]) -> F {
    x.iter().zip(y).map(|(x, y)| line(*x, *y)).product()
}

/// `eq` of one coordinate: `x * y + (1 - x) * (1 - y)`.
pub(crate) fn line(x: F, y: F) -> F {
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
            let split = SplitEq::new(&point);
            assert_eq!(split.len(), expected.len());
            let values: Vec<F> = expected.iter().map(|_| F::rand(&mut rng)).collect();
            let direct = expected
                .iter()
                .zip(&values)
                .fold(F::zero(), |sum, (eq, value)| sum + *eq * value);
            assert_eq!(split.sum(|x| values[x]), direct);
        }
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
