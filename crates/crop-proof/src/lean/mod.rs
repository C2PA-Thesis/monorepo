//! A prover for the same proof that keeps few large vectors alive. Every
//! function here writes the transcript exactly as the hyperplonk prover it
//! replaces, so the proof bytes do not change; the tests compare the two.

mod eq;
mod msm;
mod open;
mod products;
pub(crate) mod prover;
mod range;
pub(crate) mod srs;
mod sumcheck;

use ark_ff::{Field, Zero};

use crate::F;

/// The values of a polynomial over the hypercube, as the prover holds them.
/// Pixels and range counts stay bytes. A slice shorter than the hypercube
/// stands for its values followed by zeros.
#[derive(Clone, Copy)]
enum Values<'a> {
    Bytes(&'a [u8]),
    Field(&'a [F]),
}

impl Values<'_> {
    /// How many values are stored; the rest are zero.
    fn len(&self) -> usize {
        match self {
            Values::Bytes(values) => values.len(),
            Values::Field(values) => values.len(),
        }
    }
}

/// How many values a byte can take: a pixel or a count of the range check.
const BYTE_VALUES: usize = 256;

/// `step * value` for every byte value, by additions.
fn multiples(step: F) -> [F; BYTE_VALUES] {
    let mut multiple = F::zero();
    std::array::from_fn(|_| {
        let current = multiple;
        multiple += step;
        current
    })
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

/// The values of `start..start + size`, without those past the end: they are
/// zero and add nothing.
fn window<T>(values: &[T], start: usize, size: usize) -> &[T] {
    let end = values.len().min(start + size);
    values.get(start..end).unwrap_or(&[])
}

/// Writes `table` with its lowest variable fixed to `r` into its first half.
fn fold_in_place(table: &mut [F], r: F) {
    for b in 0..table.len() / 2 {
        table[b] = table[2 * b] + (table[2 * b + 1] - table[2 * b]) * r;
    }
}

/// The round polynomial of `sum of left * right` at 0, 1 and 2, for two
/// tables of the same even length.
fn product_round(left: &[F], right: &[F]) -> [F; 3] {
    let mut sums = [F::zero(); 3];
    for (left, right) in left.chunks_exact(2).zip(right.chunks_exact(2)) {
        sums[0] += left[0] * right[0];
        sums[1] += left[1] * right[1];
        sums[2] += (left[1].double() - left[0]) * (right[1].double() - right[0]);
    }
    sums
}

#[cfg(test)]
mod tests {
    use ark_ff::UniformRand;
    use rand::{rngs::StdRng, SeedableRng};

    use super::*;

    /// The sums over byte values that replace one multiplication per byte.
    #[test]
    fn additions_give_the_products_with_byte_values() {
        let mut rng = StdRng::seed_from_u64(3);
        let step = F::rand(&mut rng);
        let values: Vec<F> = (0..BYTE_VALUES).map(|_| F::rand(&mut rng)).collect();
        let mut expected = F::zero();
        for (byte, (multiple, value)) in (0..=u8::MAX).zip(multiples(step).iter().zip(&values)) {
            assert_eq!(*multiple, step * F::from(byte));
            expected += F::from(byte) * value;
        }
        assert_eq!(times_index(&values), expected);
        assert!(times_index(&[]).is_zero() && times_index(&values[..1]).is_zero());
    }
}
