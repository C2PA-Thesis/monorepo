//! Fast, self-contained tests for the lattice solver, no proving involved.

use crop_proof::lattice::solve_congruence;
use num_bigint::{BigInt, Sign};
use rand::{RngCore, SeedableRng};

fn bls_modulus() -> BigInt {
    BigInt::parse_bytes(
        b"73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001",
        16,
    )
    .unwrap()
}

fn mod_p(x: &BigInt, p: &BigInt) -> BigInt {
    let r = x % p;
    if r.sign() == Sign::Minus {
        r + p
    } else {
        r
    }
}

#[test]
fn solves_a_small_modulus_instance() {
    let p = BigInt::from(1009);
    let c: Vec<BigInt> = [12, 345, 678, 900, 111, 222].map(BigInt::from).to_vec();
    let t = BigInt::from(500);
    let delta = solve_congruence(&c, &t, &p).expect("a solution exists");
    assert_eq!(delta.len(), c.len());
    let lhs = c
        .iter()
        .zip(&delta)
        .fold(BigInt::from(0), |s, (ci, di)| s + ci * di);
    assert_eq!(mod_p(&lhs, &p), mod_p(&t, &p));
}

#[test]
fn solves_the_bls_field_with_byte_sized_deltas() {
    // Same shape as the Fiat-Shamir attack: 64 challenges in the BLS scalar
    // field, a random target, deltas that must keep a mid-range pixel in [0,255].
    let p = bls_modulus();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let k = 64;
    let mut draw = || {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        mod_p(&BigInt::from_bytes_be(Sign::Plus, &bytes), &p)
    };
    let c: Vec<BigInt> = (0..k).map(|_| draw()).collect();
    let t = draw();

    let delta = solve_congruence(&c, &t, &p).expect("a solution exists");
    assert_eq!(delta.len(), k);

    let lhs = c
        .iter()
        .zip(&delta)
        .fold(BigInt::from(0), |s, (ci, di)| s + ci * di);
    assert_eq!(mod_p(&lhs, &p), mod_p(&t, &p));

    let max_abs = delta
        .iter()
        .map(|v| {
            if v.sign() == Sign::Minus {
                -v
            } else {
                v.clone()
            }
        })
        .max()
        .unwrap();
    assert!(
        max_abs <= BigInt::from(127),
        "delta too large for a mid-range pixel: {max_abs}"
    );
}
