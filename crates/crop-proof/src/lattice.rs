//! Security analysis, attack A2: exact-integer LLL and a solver for small
//! `delta` with `sum_j c_j * delta_j = t (mod p)`, which is what substituting
//! a published image needs when the crop challenges ignore the pixels.
//!
//! The reduction is Cohen's integer LLL (A Course in Computational Algebraic
//! Number Theory, Algorithm 2.6.3): it keeps the Gram-Schmidt data as integers
//! (`d` and `lambda`), so there is no rational blow-up in the hot loop. Exact
//! rationals are used only once, after reduction, for a Babai nearest-plane
//! step. Every solution is checked against the modulus before it is returned.

// Index-driven lattice loops with cross-row access; indices are clearer here.
#![allow(clippy::needless_range_loop)]

use num_bigint::{BigInt, Sign};

/// Exact rational with a positive denominator and gcd 1. Used only in the
/// one-shot Babai step, never inside the reduction loop.
#[derive(Clone)]
struct Rat {
    n: BigInt,
    d: BigInt,
}

fn gcd(a: &BigInt, b: &BigInt) -> BigInt {
    let (mut a, mut b) = (abs(a), abs(b));
    while b.sign() != Sign::NoSign {
        let r = &a % &b;
        a = std::mem::replace(&mut b, r);
    }
    a
}

fn abs(x: &BigInt) -> BigInt {
    if x.sign() == Sign::Minus {
        -x
    } else {
        x.clone()
    }
}

/// Floor of a / b for b > 0. BigInt division truncates toward zero, so correct
/// the quotient when the remainder is negative.
fn floor_div(a: &BigInt, b: &BigInt) -> BigInt {
    let q = a / b;
    let r = a - &q * b;
    if r.sign() == Sign::Minus {
        q - 1
    } else {
        q
    }
}

/// Nearest integer to a / m for m > 0, ties rounded up.
fn nearest(a: &BigInt, m: &BigInt) -> BigInt {
    floor_div(&(a * 2 + m), &(m * 2))
}

impl Rat {
    fn new(mut n: BigInt, mut d: BigInt) -> Self {
        assert!(d.sign() != Sign::NoSign, "zero denominator");
        if d.sign() == Sign::Minus {
            n = -n;
            d = -d;
        }
        let g = gcd(&n, &d);
        if g.sign() != Sign::NoSign && g != BigInt::from(1) {
            n /= &g;
            d /= &g;
        }
        Self { n, d }
    }

    fn int(n: BigInt) -> Self {
        Self {
            n,
            d: BigInt::from(1),
        }
    }

    fn zero() -> Self {
        Self::int(BigInt::from(0))
    }

    fn add(&self, o: &Rat) -> Rat {
        Rat::new(&self.n * &o.d + &o.n * &self.d, &self.d * &o.d)
    }

    fn sub(&self, o: &Rat) -> Rat {
        Rat::new(&self.n * &o.d - &o.n * &self.d, &self.d * &o.d)
    }

    fn mul(&self, o: &Rat) -> Rat {
        Rat::new(&self.n * &o.n, &self.d * &o.d)
    }

    fn div(&self, o: &Rat) -> Rat {
        Rat::new(&self.n * &o.d, &self.d * &o.n)
    }

    /// Nearest integer, ties rounded up.
    fn round(&self) -> BigInt {
        nearest(&self.n, &self.d)
    }
}

fn dot(a: &[BigInt], b: &[BigInt]) -> BigInt {
    a.iter()
        .zip(b)
        .fold(BigInt::from(0), |sum, (x, y)| sum + x * y)
}

/// Reduced basis together with its integer Gram-Schmidt data. `d` and `lambda`
/// are 1-indexed (index 0 is padding) to match the reference algorithm.
struct Reduced {
    basis: Vec<Vec<BigInt>>,
    lambda: Vec<Vec<BigInt>>,
    d: Vec<BigInt>,
}

fn redi(b: &mut [Vec<BigInt>], lambda: &mut [Vec<BigInt>], d: &[BigInt], k: usize, l: usize) {
    if &abs(&lambda[k][l]) * 2 <= d[l] {
        return;
    }
    let q = nearest(&lambda[k][l], &d[l]);
    let dim = b[k - 1].len();
    for t in 0..dim {
        b[k - 1][t] = &b[k - 1][t] - &q * &b[l - 1][t];
    }
    lambda[k][l] = &lambda[k][l] - &q * &d[l];
    for i in 1..l {
        lambda[k][i] = &lambda[k][i] - &q * &lambda[l][i];
    }
}

fn swapi(b: &mut [Vec<BigInt>], lambda: &mut [Vec<BigInt>], d: &mut [BigInt], k: usize, n: usize) {
    b.swap(k - 1, k - 2);
    for j in 1..=k - 2 {
        let tmp = lambda[k][j].clone();
        lambda[k][j] = lambda[k - 1][j].clone();
        lambda[k - 1][j] = tmp;
    }
    let lam = lambda[k][k - 1].clone();
    let bnew = (&d[k - 2] * &d[k] + &lam * &lam) / &d[k - 1];
    for i in k + 1..=n {
        let t = lambda[i][k].clone();
        lambda[i][k] = (&d[k] * &lambda[i][k - 1] - &lam * &t) / &d[k - 1];
        lambda[i][k - 1] = (&bnew * &t + &lam * &lambda[i][k]) / &d[k];
    }
    d[k - 1] = bnew;
}

/// In-place integer LLL with delta = 3/4. Returns the reduced basis and its
/// Gram-Schmidt data.
fn lll_integer(mut b: Vec<Vec<BigInt>>) -> Reduced {
    let n = b.len();
    let mut d = vec![BigInt::from(0); n + 1];
    d[0] = BigInt::from(1);
    let mut lambda = vec![vec![BigInt::from(0); n + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=i {
            let mut u = dot(&b[i - 1], &b[j - 1]);
            for l in 1..j {
                u = (&d[l] * &u - &lambda[i][l] * &lambda[j][l]) / &d[l - 1];
            }
            if j < i {
                lambda[i][j] = u;
            } else {
                d[i] = u;
            }
        }
    }

    let mut k = 2;
    while k <= n {
        redi(&mut b, &mut lambda, &d, k, k - 1);
        // Lovasz condition (no swap): 4 d_k d_{k-2} >= 3 d_{k-1}^2 - 4 lambda^2.
        let lhs = BigInt::from(4) * &d[k] * &d[k - 2];
        let rhs = BigInt::from(3) * &d[k - 1] * &d[k - 1]
            - BigInt::from(4) * &lambda[k][k - 1] * &lambda[k][k - 1];
        if lhs >= rhs {
            for l in (1..=k - 2).rev() {
                redi(&mut b, &mut lambda, &d, k, l);
            }
            k += 1;
        } else {
            swapi(&mut b, &mut lambda, &mut d, k, n);
            if k > 2 {
                k -= 1;
            }
        }
    }
    Reduced {
        basis: b,
        lambda,
        d,
    }
}

fn mod_p(x: &BigInt, p: &BigInt) -> BigInt {
    let r = x % p;
    if r.sign() == Sign::Minus {
        r + p
    } else {
        r
    }
}

/// Modular inverse of `a` mod prime `p`.
fn mod_inv(a: &BigInt, p: &BigInt) -> BigInt {
    let (mut old_r, mut r) = (mod_p(a, p), p.clone());
    let (mut old_s, mut s) = (BigInt::from(1), BigInt::from(0));
    while r.sign() != Sign::NoSign {
        let q = floor_div(&old_r, &r);
        let nr = &old_r - &q * &r;
        old_r = std::mem::replace(&mut r, nr);
        let ns = &old_s - &q * &s;
        old_s = std::mem::replace(&mut s, ns);
    }
    mod_p(&old_s, p)
}

/// Explicit Gram-Schmidt vectors b*_i = b_i - sum_{j<i} mu[i][j] b*_j, computed
/// once from the reduced basis for the Babai step.
fn gram_schmidt_vectors(basis: &[Vec<BigInt>], mu: &[Vec<Rat>]) -> Vec<Vec<Rat>> {
    let n = basis.len();
    let mut bstar: Vec<Vec<Rat>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut v: Vec<Rat> = basis[i].iter().map(|x| Rat::int(x.clone())).collect();
        for (j, bstar_j) in bstar.iter().enumerate() {
            for (vc, g) in v.iter_mut().zip(bstar_j) {
                *vc = vc.sub(&mu[i][j].mul(g));
            }
        }
        bstar.push(v);
    }
    bstar
}

/// Finds small integers `delta` with `sum_j c[j] * delta[j] = t (mod p)`.
///
/// Builds the kernel lattice `{x : c.x = 0 (mod p)}`, a determinant-`p` lattice
/// whose entries stay near `p`, reduces it, then runs Babai nearest-plane on a
/// particular solution to pull it down to a short coset representative. The
/// result is verified mod `p` before returning.
pub fn solve_congruence(c: &[BigInt], t: &BigInt, p: &BigInt) -> Option<Vec<BigInt>> {
    let k = c.len();
    let pivot = c
        .iter()
        .position(|cj| mod_p(cj, p).sign() != Sign::NoSign)?;
    let inv = mod_inv(&c[pivot], p);

    // Kernel basis: p at the pivot, and e_j + a_j e_pivot otherwise, with
    // a_j = -c_j / c_pivot (mod p). Each row satisfies c.row = 0 (mod p).
    let mut basis = vec![vec![BigInt::from(0); k]; k];
    basis[0][pivot] = p.clone();
    let mut row = 1;
    for (j, cj) in c.iter().enumerate() {
        if j == pivot {
            continue;
        }
        basis[row][j] = BigInt::from(1);
        basis[row][pivot] = mod_p(&(-(cj * &inv)), p);
        row += 1;
    }

    let reduced = lll_integer(basis);
    let Reduced { basis, lambda, d } = reduced;

    // mu[i][j] = lambda[i+1][j+1] / d[j+1], b_norm[i] = d[i+1] / d[i].
    let mut mu = vec![vec![Rat::zero(); k]; k];
    for i in 0..k {
        for j in 0..i {
            mu[i][j] = Rat::new(lambda[i + 1][j + 1].clone(), d[j + 1].clone());
        }
    }
    let bstar = gram_schmidt_vectors(&basis, &mu);
    let b_norm: Vec<Rat> = (0..k)
        .map(|i| Rat::new(d[i + 1].clone(), d[i].clone()))
        .collect();

    // Particular solution, then Babai nearest-plane.
    let mut delta: Vec<BigInt> = vec![BigInt::from(0); k];
    delta[pivot] = mod_p(&(t * &inv), p);
    for i in (0..k).rev() {
        let mut num = Rat::zero();
        for (dv, g) in delta.iter().zip(&bstar[i]) {
            num = num.add(&Rat::int(dv.clone()).mul(g));
        }
        let coeff = num.div(&b_norm[i]).round();
        if coeff.sign() != Sign::NoSign {
            for (dv, bv) in delta.iter_mut().zip(&basis[i]) {
                *dv = &*dv - &coeff * bv;
            }
        }
    }

    (mod_p(&dot(c, &delta), p) == mod_p(t, p)).then_some(delta)
}
