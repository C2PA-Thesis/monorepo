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

#[cfg(test)]
mod tests {
    use arithmetic::build_eq_x_r_vec;
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
}
