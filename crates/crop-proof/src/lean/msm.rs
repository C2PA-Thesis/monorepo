//! Multi-scalar multiplications over a basis of the commitment key, in
//! bounded memory.
//!
//! Both are bucket methods: a point is added to the bucket of its scalar, or
//! of one digit of its scalar, and each bucket is multiplied once at the
//! end. The buckets are the only state, so the scalars and the points can be
//! read in pieces and forgotten.

use std::ops::Add;

use ark_bls12_381::{Fq, G1Affine, G1Projective};
use ark_ec::Group;
use ark_ff::{Field, One, PrimeField, Zero};
use rayon::prelude::*;

use super::srs::Basis;
use crate::F;

/// Scalars `msm_with` recodes before it adds their points. The digits and,
/// for a mapped basis, the decoded points of one chunk are alive at a time:
/// 138 bytes per scalar at the widest window, 4.5 MB in all. At 2^20 scalars
/// a chunk of 2^15 took 5% less time than one of 2^14 and 20% less than one
/// of 2^12; 2^16 gained 1% more.
const CHUNK: usize = 1 << 15;

/// Scalars one task asks `fill` for at a time. They sit on its stack.
const PIECE: usize = 256;

/// The widest window of `msm_with`. A window of w bits has 2^(w-1) buckets
/// of 104 bytes, so 15 bits and their 17 windows hold 29 MB of buckets.
const MAX_WINDOW: usize = 15;
// The digits are stored as i16.
const _: () = assert!(MAX_WINDOW <= 15);

/// Additions that share one field inversion. An inversion costs about 300
/// multiplications, so a batch this long pays a third of one per addition.
/// A longer batch meets more buckets twice; 512 and 2048 measured 2% to 4%
/// slower. It is also how many additions wait for a busy bucket before they
/// are retried, which bounds every queue of a `Batch`.
const BATCH: usize = 1 << 10;
// A bucket's sum aside is found through a u16.
const _: () = assert!(BATCH <= u16::MAX as usize);

/// Bits of a scalar once those above half the modulus are negated.
const MAGNITUDE_BITS: usize = F::MODULUS_BIT_SIZE as usize - 1;

/// Tasks `msm_bytes` splits its scalars into at most, and the fewest scalars
/// worth a task: each task ends with 510 additions of its own.
const BYTE_TASKS: usize = 64;
const BYTE_TASK: usize = 1 << 12;

/// `sum of scalars[i] * basis[i]`. Scalars past the end of the slice are zero.
///
/// A scalar is a byte, so there are 255 buckets, one addition per non-zero
/// scalar, and nothing is allocated in proportion to the input.
pub(crate) fn msm_bytes(basis: &Basis, scalars: &[u8]) -> G1Projective {
    assert!(
        scalars.len() <= basis.len(),
        "more scalars than basis points"
    );
    let task = scalars.len().div_ceil(BYTE_TASKS).max(BYTE_TASK);
    scalars
        .par_chunks(task)
        .enumerate()
        .map(|(index, scalars)| {
            let start = index * task;
            let mut buckets = vec![G1Projective::zero(); usize::from(u8::MAX)];
            for (offset, &scalar) in scalars.iter().enumerate() {
                if let Some(bucket) = usize::from(scalar).checked_sub(1) {
                    buckets[bucket] += basis.point(start + offset).as_ref();
                }
            }
            weighted_sum(&buckets)
        })
        .sum()
}

/// `sum of scalars[i] * basis[i]`. Scalars past the end of the slice are zero.
pub(crate) fn msm(basis: &Basis, scalars: &[F]) -> G1Projective {
    msm_with(basis, scalars.len(), |start, out| {
        out.copy_from_slice(&scalars[start..start + out.len()])
    })
}

/// `sum of scalar(i) * basis[i]` for `i < len`, where `fill(start, out)`
/// writes the scalars of `start..start + out.len()`. It is called once per
/// index, in any order and from several threads.
pub(crate) fn msm_with(
    basis: &Basis,
    len: usize,
    fill: impl Fn(usize, &mut [F]) + Sync,
) -> G1Projective {
    pippenger(basis, len, CHUNK, fill)
}

/// Pippenger's method over `chunk` scalars at a time.
///
/// Every scalar is cut into signed digits, one per window, and each window
/// has its own buckets, which one task fills. The buckets live across the
/// chunks, so the window is as wide as for the whole input and the chunks
/// cost no extra additions.
///
/// Memory is the buckets, one chunk of digits and decoded points, and one
/// `Batch` per window. All of it is kept from chunk to chunk instead of
/// allocated again, and none of it depends on the number of threads. A
/// `Batch` holds 0.2 MB for scalars that spread over the buckets and at
/// most 0.5 MB for scalars that crowd them, so the 17 windows of 2^20
/// scalars hold 3 MB to 8 MB.
///
/// A scalar equal to one has no digits: its point goes to a sum of its own.
/// The fraction polynomial of the range check is one at a quarter of its
/// points for a full image, and at more for a padded one. All those points
/// would wait on a single bucket.
fn pippenger(
    basis: &Basis,
    len: usize,
    chunk: usize,
    fill: impl Fn(usize, &mut [F]) + Sync,
) -> G1Projective {
    assert!(len <= basis.len(), "more scalars than basis points");
    if len == 0 {
        return G1Projective::zero();
    }
    let window = window_bits(len);
    let windows = window_count(window);
    let per_window = 1 << (window - 1);
    let mut buckets = vec![G1Affine::identity(); windows * per_window];
    let mut batches: Vec<Batch> = (0..windows)
        .map(|_| Batch::new(per_window, chunk.min(len)))
        .collect();
    let mut digits = vec![0i16; chunk.min(len) * windows];
    let mut decoded = Vec::new();
    let mut ones = G1Projective::zero();
    for start in (0..len).step_by(chunk) {
        let end = len.min(start + chunk);
        let points = basis.read(start..end, &mut decoded);
        let digits = &mut digits[..(end - start) * windows];
        ones += digits
            .par_chunks_mut(PIECE * windows)
            .zip(points.par_chunks(PIECE))
            .enumerate()
            .map(|(piece, (digits, points))| {
                let mut scalars = [F::zero(); PIECE];
                let scalars = &mut scalars[..points.len()];
                fill(start + piece * PIECE, scalars);
                let mut ones = G1Projective::zero();
                let digits = digits.chunks_exact_mut(windows);
                for ((scalar, digits), point) in scalars.iter().zip(digits).zip(points) {
                    if scalar.is_one() {
                        ones += point;
                        digits.fill(0);
                    } else {
                        recode(*scalar, window, digits);
                    }
                }
                ones
            })
            .sum::<G1Projective>();
        let digits = &*digits;
        buckets
            .par_chunks_mut(per_window)
            .zip(batches.par_iter_mut())
            .enumerate()
            .for_each(|(index, (buckets, batch))| {
                for (point, digits) in digits.chunks_exact(windows).enumerate() {
                    let digit = digits[index];
                    // Bucket i holds the points to multiply by i + 1.
                    if let Some(bucket) = usize::from(digit.unsigned_abs()).checked_sub(1) {
                        let addition = Addition {
                            bucket,
                            point,
                            negative: digit < 0,
                        };
                        batch.add(addition, buckets, points);
                    }
                }
                batch.finish(buckets, points);
            });
    }
    let sums: Vec<G1Projective> = buckets.par_chunks(per_window).map(weighted_sum).collect();
    // Horner's rule in 2^window, from the highest window down.
    let weighted = sums
        .iter()
        .rev()
        .fold(G1Projective::zero(), |mut total, sum| {
            for _ in 0..window {
                total.double_in_place();
            }
            total + sum
        });
    weighted + ones
}

/// `buckets[bucket] += points[point]`, or `-=` if `negative`.
#[derive(Clone, Copy)]
struct Addition {
    bucket: usize,
    point: usize,
    negative: bool,
}

impl Addition {
    fn point(&self, points: &[G1Affine]) -> G1Affine {
        let point = points[self.point];
        if self.negative {
            -point
        } else {
            point
        }
    }
}

/// Additions to buckets kept in affine form. Adding two affine points needs
/// the inverse of a field element, and one inversion serves a whole batch of
/// additions as long as each is to a different bucket. With it an addition
/// costs 6 multiplications where adding to a projective bucket costs 11.
///
/// An addition to a bucket already in the batch waits, and is retried once
/// `BATCH` of them wait. If its bucket is taken again, many scalars share
/// that digit, and waiting would cost a batch per addition: such points are
/// summed aside in projective form and that sum is added to the bucket later.
///
/// Every queue holds at most `BATCH` entries whatever the scalars, so a
/// `Batch` is at most 0.5 MB at the widest window. Runs of equal scalars,
/// which a flat area of an image gives, would otherwise make the queues as
/// long as a chunk.
struct Batch {
    /// The additions of the batch and the divisors of their slopes.
    additions: Vec<Addition>,
    divisors: Vec<Fq>,
    /// Which buckets the batch adds to.
    busy: Vec<bool>,
    /// Additions that found their bucket busy once.
    later: Vec<Addition>,
    /// The sums kept aside, each with its bucket, and for every bucket one
    /// more than the index of its sum, or zero.
    aside: Vec<(usize, G1Projective)>,
    slots: Vec<u16>,
    /// Scratch: the sums kept aside in affine form, and for `invert`.
    settled: Vec<G1Affine>,
    prefixes: Vec<Fq>,
}

impl Batch {
    /// A batch for `buckets` buckets that gets `chunk` additions at a time.
    /// The queues every input fills are allocated here, once; the sums aside
    /// only take room when scalars crowd a bucket.
    fn new(buckets: usize, chunk: usize) -> Self {
        let queue = BATCH.min(chunk);
        Self {
            additions: Vec::with_capacity(queue),
            divisors: Vec::with_capacity(queue),
            busy: vec![false; buckets],
            later: Vec::with_capacity(queue),
            aside: Vec::new(),
            slots: vec![0; buckets],
            settled: Vec::new(),
            prefixes: Vec::with_capacity(queue),
        }
    }

    fn add(&mut self, addition: Addition, buckets: &mut [G1Affine], points: &[G1Affine]) {
        if !self.schedule(addition, buckets, points) {
            self.later.push(addition);
            if self.later.len() == BATCH {
                self.retry(buckets, points);
            }
        }
    }

    /// Puts the addition in the batch, or performs it if it needs no slope.
    /// Returns false, and does nothing, if its bucket is busy.
    fn schedule(
        &mut self,
        addition: Addition,
        buckets: &mut [G1Affine],
        points: &[G1Affine],
    ) -> bool {
        if self.busy[addition.bucket] {
            return false;
        }
        let point = addition.point(points);
        let bucket = &mut buckets[addition.bucket];
        if point.infinity {
            return true;
        }
        if bucket.infinity {
            *bucket = point;
            return true;
        }
        let divisor = if bucket.x != point.x {
            point.x - bucket.x
        } else if bucket.y == point.y && !point.y.is_zero() {
            // The same point twice: the slope is that of the tangent.
            point.y.double()
        } else {
            *bucket = G1Affine::identity();
            return true;
        };
        self.busy[addition.bucket] = true;
        self.additions.push(addition);
        self.divisors.push(divisor);
        if self.additions.len() == BATCH {
            self.flush(buckets, points);
        }
        true
    }

    /// Performs the additions of the batch.
    fn flush(&mut self, buckets: &mut [G1Affine], points: &[G1Affine]) {
        invert(&mut self.divisors, &mut self.prefixes);
        for (addition, inverse) in self.additions.drain(..).zip(self.divisors.drain(..)) {
            let point = addition.point(points);
            let bucket = &mut buckets[addition.bucket];
            let rise = if bucket.x != point.x {
                point.y - bucket.y
            } else {
                // The curve is y^2 = x^3 + 4, so the tangent rises by 3 x^2.
                let square = bucket.x.square();
                square.double() + square
            };
            let slope = rise * inverse;
            let x = slope.square() - bucket.x - point.x;
            let y = slope * (bucket.x - x) - bucket.y;
            *bucket = G1Affine::new_unchecked(x, y);
            self.busy[addition.bucket] = false;
        }
    }

    /// Takes up the additions that wait: each goes into a batch, or onto the
    /// sum aside of its bucket if the bucket is busy again.
    fn retry(&mut self, buckets: &mut [G1Affine], points: &[G1Affine]) {
        self.flush(buckets, points);
        let later = std::mem::take(&mut self.later);
        for &addition in &later {
            if !self.schedule(addition, buckets, points) {
                let slot = &mut self.slots[addition.bucket];
                if *slot == 0 {
                    self.aside.push((addition.bucket, G1Projective::zero()));
                    // Fewer than `BATCH` sums are aside, see below.
                    *slot = self.aside.len() as u16;
                }
                self.aside[usize::from(*slot) - 1].1 += addition.point(points);
            }
        }
        self.later = later;
        self.later.clear();
        // A sum aside starts with the second addition to a bucket among at
        // most `BATCH` that waited, so one call starts at most half a batch
        // of them, and with this limit there are never more than a batch.
        if self.aside.len() >= BATCH / 2 {
            self.settle(buckets, points);
        }
    }

    /// Adds the sums kept aside to their buckets. Brought to affine form
    /// with one inversion, they are one more addition each, all to different
    /// buckets.
    fn settle(&mut self, buckets: &mut [G1Affine], points: &[G1Affine]) {
        self.flush(buckets, points);
        for (bucket, _) in &self.aside {
            self.slots[*bucket] = 0;
        }
        self.aside.retain(|(_, sum)| !sum.is_zero());
        self.divisors
            .extend(self.aside.iter().map(|(_, sum)| sum.z));
        invert(&mut self.divisors, &mut self.prefixes);
        let mut settled = std::mem::take(&mut self.settled);
        settled.extend(self.aside.iter().zip(self.divisors.drain(..)).map(
            |((_, sum), inverse)| {
                let square = inverse.square();
                G1Affine::new_unchecked(sum.x * square, sum.y * square * inverse)
            },
        ));
        for point in 0..settled.len() {
            let addition = Addition {
                bucket: self.aside[point].0,
                point,
                negative: false,
            };
            let scheduled = self.schedule(addition, buckets, &settled);
            debug_assert!(scheduled, "two sums aside for one bucket");
        }
        self.flush(buckets, &settled);
        self.aside.clear();
        settled.clear();
        self.settled = settled;
    }

    /// Performs every addition still waiting, at the end of a chunk: the
    /// points of the next one will not be these.
    fn finish(&mut self, buckets: &mut [G1Affine], points: &[G1Affine]) {
        self.retry(buckets, points);
        self.settle(buckets, points);
    }
}

/// Inverts every value in place with a single field inversion: the inverse
/// of the product of all of them, peeled off one factor at a time.
fn invert(values: &mut [Fq], prefixes: &mut Vec<Fq>) {
    if values.is_empty() {
        return;
    }
    prefixes.clear();
    let mut product = Fq::ONE;
    for value in values.iter() {
        prefixes.push(product);
        product *= value;
    }
    // A divisor is a difference of two different coordinates or twice a
    // non-zero one, so the product is not zero.
    let mut inverse = product.inverse().unwrap_or_default();
    for (value, prefix) in values.iter_mut().zip(prefixes.iter()).rev() {
        let next = inverse * *value;
        *value = inverse * prefix;
        inverse = next;
    }
}

/// `sum of (i + 1) * buckets[i]` in two additions per bucket: the running
/// sum from the top is added once for every factor it still has to gain.
fn weighted_sum<P>(buckets: &[P]) -> G1Projective
where
    G1Projective: for<'a> Add<&'a P, Output = G1Projective>,
{
    let mut running = G1Projective::zero();
    let mut total = G1Projective::zero();
    for bucket in buckets.iter().rev() {
        running = running + bucket;
        total += running;
    }
    total
}

/// The window for `len` scalars: the one with the least `windows * (scalars
/// + buckets)`, and the widest of those.
///
/// That counts one addition per scalar and one per bucket in every window.
/// Summing the buckets really costs two each, but a wider window also
/// spreads the scalars over more buckets, so fewer additions of a chunk meet
/// in the same bucket. Counting a bucket once picked the fastest window
/// measured from 2^16 to 2^20 scalars, where the time goes, and one within
/// 15% of the fastest from 2^10 to 2^15.
fn window_bits(len: usize) -> usize {
    (1..=MAX_WINDOW)
        .rev()
        .min_by_key(|&window| window_count(window) * (len + (1 << (window - 1))))
        .unwrap_or(MAX_WINDOW)
}

/// Digits of `window` bits in a scalar. The last one has at least one bit to
/// spare, so it takes the carry of the one below without overflowing.
fn window_count(window: usize) -> usize {
    MAGNITUDE_BITS / window + 1
}

/// Writes `scalar` as `sum of digits[i] * 2^(i * window)` with digits of
/// absolute value at most `2^(window - 1)`, which halves the buckets.
fn recode(scalar: F, window: usize, digits: &mut [i16]) {
    // A scalar above half the modulus is the negation of a shorter one. Its
    // digits are those of the shorter one with the signs flipped.
    let negative = scalar.into_bigint() > F::MODULUS_MINUS_ONE_DIV_TWO;
    let limbs = if negative { -scalar } else { scalar }.into_bigint().0;
    let half = 1i32 << (window - 1);
    let mut carry = 0;
    let last = digits.len() - 1;
    for (index, digit) in digits.iter_mut().enumerate() {
        let mut value = bits(&limbs, index * window, window) + carry;
        // A digit of the upper half becomes a negative one and a carry.
        carry = i32::from(index < last && value >= half);
        value -= carry << window;
        let signed = if negative { -value } else { value };
        // At most 2^14 in absolute value, since a window has at most 15 bits.
        *digit = signed as i16;
    }
}

/// The `width` bits of `limbs` from bit `offset` up.
fn bits(limbs: &[u64; 4], offset: usize, width: usize) -> i32 {
    let (limb, shift) = (offset / 64, offset % 64);
    let mut value = limbs[limb] >> shift;
    if shift + width > 64 && limb + 1 < limbs.len() {
        value |= limbs[limb + 1] << (64 - shift);
    }
    (value & ((1 << width) - 1)) as i32
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering::Relaxed};

    use ark_bls12_381::G1Affine;
    use ark_ec::{CurveGroup, VariableBaseMSM};
    use ark_ff::{Field, One, UniformRand};
    use ark_serialize::CanonicalSerialize;
    use rand::{Rng, SeedableRng};
    use subroutines::pcs::prelude::PolynomialCommitmentScheme;

    use super::*;
    use crate::{
        iop::poly,
        lean::srs::{tests as srs, Basis, Srs},
        Pcs,
    };

    const NUM_VARS: usize = 13;
    const LENGTHS: [usize; 9] = [0, 1, 2, 255, 257, 1000, 5000, 8191, 8192];

    fn rng() -> rand::rngs::StdRng {
        rand::rngs::StdRng::seed_from_u64(11)
    }

    /// What arkworks computes for the same scalars and points.
    fn expected(points: &[G1Affine], scalars: &[F]) -> G1Projective {
        G1Projective::msm_unchecked(&points[..scalars.len()], scalars)
    }

    /// Scalars at the edges of the recoding, then random ones.
    fn scalars(len: usize, rng: &mut impl Rng) -> Vec<F> {
        let half = F::from_bigint(F::MODULUS_MINUS_ONE_DIV_TWO).unwrap();
        let edges = [
            F::zero(),
            F::one(),
            -F::one(),
            half,
            half + F::one(),
            F::from(2u8).pow([253]),
            F::from(2u8).pow([254]) - F::one(),
            F::from(u64::MAX),
            -F::from(u64::MAX),
        ];
        let mut scalars: Vec<F> = edges.into_iter().take(len).collect();
        scalars.resize_with(len, || F::rand(rng));
        scalars
    }

    #[test]
    fn digits_add_up_to_the_scalar() {
        let mut rng = rng();
        for window in 1..=MAX_WINDOW {
            let mut digits = vec![0i16; window_count(window)];
            let radix = F::from(1u64 << window);
            for scalar in scalars(64, &mut rng) {
                recode(scalar, window, &mut digits);
                let limit = 1 << (window - 1);
                assert!(digits.iter().all(|digit| digit.unsigned_abs() <= limit));
                let value = digits.iter().rev().fold(F::zero(), |value, &digit| {
                    let magnitude = F::from(digit.unsigned_abs());
                    value * radix + if digit < 0 { -magnitude } else { magnitude }
                });
                assert_eq!(value, scalar, "window {window}");
            }
        }
    }

    #[test]
    fn byte_scalars_match_arkworks() {
        let params = srs::params(NUM_VARS);
        let (_file, mapped) = srs::mapped(&params);
        let points = &params.powers_of_g[0].evals;
        let mut rng = rng();
        for srs in [Srs::Loaded(params.clone()), mapped] {
            let basis = srs.basis(NUM_VARS);
            for len in LENGTHS {
                let random: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                for bytes in [random, vec![0; len], vec![255; len]] {
                    let lifted: Vec<F> = bytes.iter().map(|&byte| F::from(byte)).collect();
                    assert_eq!(msm_bytes(&basis, &bytes), expected(points, &lifted));
                }
            }
        }
    }

    #[test]
    fn field_scalars_match_arkworks() {
        let params = srs::params(NUM_VARS);
        let (_file, mapped) = srs::mapped(&params);
        let points = &params.powers_of_g[0].evals;
        let mut rng = rng();
        for srs in [Srs::Loaded(params.clone()), mapped] {
            let basis = srs.basis(NUM_VARS);
            for len in LENGTHS {
                let scalars = scalars(len, &mut rng);
                let expected = expected(points, &scalars);
                assert_eq!(msm(&basis, &scalars), expected, "length {len}");
                // Chunks that end inside a piece, and that do not divide the
                // length.
                for chunk in [100, 256, 1000, 4096] {
                    let fill = |start: usize, out: &mut [F]| {
                        out.copy_from_slice(&scalars[start..start + out.len()])
                    };
                    assert_eq!(pippenger(&basis, len, chunk, fill), expected);
                }
            }
        }
    }

    /// Scalars that send many points to the same bucket.
    fn crowded(len: usize, rng: &mut impl Rng) -> Vec<Vec<F>> {
        let value = F::rand(rng);
        let runs: Vec<F> = (0..len.div_ceil(3)).map(|_| F::rand(rng)).collect();
        vec![
            vec![F::one(); len],
            vec![-F::one(); len],
            vec![value; len],
            (0..len)
                .map(|i| if i % 2 == 0 { value } else { -value })
                .collect(),
            (0..len).map(|_| F::from(rng.gen_range(0..3u8))).collect(),
            (0..len).map(|_| F::from(rng.gen::<u16>())).collect(),
            (0..len).map(|_| -F::from(rng.gen::<u8>())).collect(),
            // Runs of three: the second waits and the third goes aside, so
            // the sums aside are many and are settled before the chunk ends.
            (0..len).map(|i| runs[i / 3]).collect(),
        ]
    }

    #[test]
    fn crowded_buckets_match_arkworks() {
        let params = srs::params(NUM_VARS);
        let points = &params.powers_of_g[0].evals;
        let basis = Basis::Points(points);
        let mut rng = rng();
        for len in [1, 2, 3, 300, 8192] {
            for scalars in crowded(len, &mut rng) {
                let expected = expected(points, &scalars);
                assert_eq!(msm(&basis, &scalars), expected, "length {len}");
                let fill = |start: usize, out: &mut [F]| {
                    out.copy_from_slice(&scalars[start..start + out.len()])
                };
                assert_eq!(pippenger(&basis, len, 700, fill), expected);
            }
        }
    }

    #[test]
    fn repeated_and_opposite_points_match_arkworks() {
        // A key never repeats a point, but the additions must not rely on it:
        // a bucket then meets its own point, its negation and infinity.
        let params = srs::params(6);
        let mut rng = rng();
        let points: Vec<G1Affine> = (0..2048)
            .map(|i| {
                let point = params.powers_of_g[0].evals[rng.gen_range(0..4)];
                match i % 5 {
                    0 | 1 => point,
                    2 | 3 => -point,
                    _ => G1Affine::identity(),
                }
            })
            .collect();
        let basis = Basis::Points(&points);
        let mut cases = crowded(points.len(), &mut rng);
        cases.push(scalars(points.len(), &mut rng));
        for scalars in cases {
            let expected = expected(&points, &scalars);
            assert_eq!(msm(&basis, &scalars), expected);
            let bytes: Vec<u8> = (0..points.len()).map(|_| rng.gen_range(0..4)).collect();
            let lifted: Vec<F> = bytes.iter().map(|&byte| F::from(byte)).collect();
            assert_eq!(msm_bytes(&basis, &bytes), self::expected(&points, &lifted));
        }
    }

    /// `len` different points: the running sums of a key's points. A key
    /// that large would take a long time to make.
    fn running_sums(len: usize) -> Vec<G1Affine> {
        let key = srs::params(NUM_VARS).powers_of_g.swap_remove(0).evals;
        let mut sum = G1Projective::zero();
        let sums: Vec<G1Projective> = key
            .iter()
            .cycle()
            .take(len)
            .map(|point| {
                sum += point;
                sum
            })
            .collect();
        G1Projective::normalize_batch(&sums)
    }

    #[test]
    fn several_chunks_of_the_widest_window_match_arkworks() {
        // What the prover runs and the other tests are too short for: digits
        // of 15 bits, and three chunks of which the last is not whole.
        let len = 2 * CHUNK + 5;
        assert_eq!(window_bits(len), MAX_WINDOW);
        let points = running_sums(len);
        let mut file = Vec::new();
        for point in &points {
            point.serialize_uncompressed(&mut file).unwrap();
        }
        let mut rng = rng();
        let mut scalars = scalars(len, &mut rng);
        // The shapes of a range check: ones at a quarter of the points, one
        // value across the end of a chunk, and runs of three.
        for scalar in scalars[1000..9000].iter_mut().step_by(4) {
            *scalar = F::one();
        }
        let value = scalars[CHUNK - 1500];
        scalars[CHUNK - 1500..CHUNK + 1500].fill(value);
        for run in scalars[CHUNK + 6000..CHUNK + 12_000].chunks_mut(3) {
            run.fill(run[0]);
        }
        let expected = expected(&points, &scalars);
        let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        let lifted: Vec<F> = bytes.iter().map(|&byte| F::from(byte)).collect();
        let expected_bytes = self::expected(&points, &lifted);
        for basis in [Basis::Points(&points), Basis::Mapped(&file)] {
            assert_eq!(msm(&basis, &scalars), expected);
            assert_eq!(msm_bytes(&basis, &bytes), expected_bytes);
        }
    }

    #[test]
    fn every_scalar_is_asked_for_once() {
        let params = srs::params(NUM_VARS);
        let srs = Srs::Loaded(params);
        let mut rng = rng();
        for len in [1, 300, 5000] {
            let scalars = scalars(len, &mut rng);
            let asked: Vec<AtomicU8> = (0..len).map(|_| AtomicU8::new(0)).collect();
            let result = pippenger(&srs.basis(NUM_VARS), len, 1000, |start, out| {
                for (offset, scalar) in out.iter_mut().enumerate() {
                    asked[start + offset].fetch_add(1, Relaxed);
                    *scalar = scalars[start + offset];
                }
            });
            assert_eq!(result, msm(&srs.basis(NUM_VARS), &scalars));
            assert!(asked.iter().all(|count| count.load(Relaxed) == 1));
        }
    }

    #[test]
    fn commits_as_hyperplonk() {
        let params = srs::params(10);
        let (_file, mapped) = srs::mapped(&params);
        let mut rng = rng();
        for srs in [Srs::Loaded(params.clone()), mapped] {
            for num_vars in 0..=10 {
                let basis = srs.basis(num_vars);
                // A slice shorter than the hypercube stands for zero padding.
                for len in [1 << num_vars, (1 << num_vars) * 2 / 3] {
                    let scalars = scalars(len, &mut rng);
                    let mut padded = scalars.clone();
                    padded.resize(1 << num_vars, F::zero());
                    let commitment = Pcs::commit(&params, &poly(padded)).unwrap();
                    assert_eq!(msm(&basis, &scalars).into_affine(), commitment.0);

                    let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
                    let mut padded: Vec<F> = bytes.iter().map(|&byte| F::from(byte)).collect();
                    padded.resize(1 << num_vars, F::zero());
                    let commitment = Pcs::commit(&params, &poly(padded)).unwrap();
                    assert_eq!(msm_bytes(&basis, &bytes).into_affine(), commitment.0);
                }
            }
        }
    }
}
