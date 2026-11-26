//! Sumcheck over tables held in full, folded in place. The general case:
//! any sum of products of multilinear polynomials.

use ark_ff::Zero;
use rayon::prelude::*;

use super::sumcheck::Rounds;
use crate::F;

/// `sum of coefficient * product of tables[indices]`, owning its tables.
pub(crate) struct Dense {
    num_vars: usize,
    max_degree: usize,
    tables: Vec<Vec<F>>,
    products: Vec<(F, Vec<usize>)>,
}

impl Dense {
    /// Every table has `2^num_vars` values; each product lists the tables it
    /// multiplies.
    pub(crate) fn new(
        num_vars: usize,
        tables: Vec<Vec<F>>,
        products: Vec<(F, Vec<usize>)>,
    ) -> Self {
        assert!(tables.iter().all(|table| table.len() == 1 << num_vars));
        let max_degree = products
            .iter()
            .map(|(_, indices)| indices.len())
            .max()
            .unwrap_or(0);
        Self {
            num_vars,
            max_degree,
            tables,
            products,
        }
    }
}

/// `table` with its lowest variable fixed to `r`, in half the space.
pub(crate) fn fold(table: &[F], r: F) -> Vec<F> {
    (0..table.len() / 2)
        .into_par_iter()
        .map(|b| table[2 * b] + (table[2 * b + 1] - table[2 * b]) * r)
        .collect()
}

impl Rounds for Dense {
    fn num_vars(&self) -> usize {
        self.num_vars
    }

    fn max_degree(&self) -> usize {
        self.max_degree
    }

    fn round(&mut self, challenge: Option<F>) -> Vec<F> {
        if let Some(r) = challenge {
            // One table at a time, so only one half-size copy exists at once.
            for table in &mut self.tables {
                *table = fold(table, r);
            }
        }
        let tables = &self.tables;
        let half = tables.first().map_or(0, |table| table.len() / 2);
        let points = self.max_degree + 1;
        let mut total = vec![F::zero(); points];
        for (coefficient, indices) in &self.products {
            // A product of k tables has degree k; evaluating it past k + 1
            // points gives what extrapolating from them would.
            let sums = (0..half)
                .into_par_iter()
                .fold(
                    || {
                        (
                            vec![(F::zero(), F::zero()); indices.len()],
                            vec![F::zero(); points],
                        )
                    },
                    |(mut lines, mut sums), b| {
                        for (line, &index) in lines.iter_mut().zip(indices) {
                            let table = &tables[index];
                            *line = (table[2 * b], table[2 * b + 1] - table[2 * b]);
                        }
                        for sum in &mut sums {
                            *sum += lines.iter().map(|(value, _)| value).product::<F>();
                            for (value, step) in &mut lines {
                                *value += *step;
                            }
                        }
                        (lines, sums)
                    },
                )
                .map(|(_, sums)| sums)
                .reduce(
                    || vec![F::zero(); points],
                    |mut left, right| {
                        for (sum, value) in left.iter_mut().zip(right) {
                            *sum += value;
                        }
                        left
                    },
                );
            for (sum, value) in total.iter_mut().zip(sums) {
                *sum += value * coefficient;
            }
        }
        total
    }
}
