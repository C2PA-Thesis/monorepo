//! A2 against upstream HyperVerITAS: given the verifier's crop challenges,
//! the honest crop and an unrelated target, adjust 64 target pixels per
//! channel so the weighted sum matches the honest one mod p. Same method as
//! substitute_crop in crates/crop-proof/src/forge.rs, with the same solver.
#[path = "../../../../../crates/crop-proof/src/lattice.rs"]
mod lattice;

use num_bigint::{BigInt, Sign};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[allow(non_snake_case)]
struct Image { rows: usize, cols: usize, R: Vec<u8>, G: Vec<u8>, B: Vec<u8> }

const ATTEMPTS: usize = 8;

fn corrective_pixels(attempt: usize) -> Vec<usize> {
    (0..64).map(|j| 97 * j + 13 + 7 * attempt).collect()
}

fn claim(r: &[BigInt], px: &[u8], p: &BigInt) -> BigInt {
    let mut acc = BigInt::from(0);
    for (c, &v) in r.iter().zip(px) { acc += c * BigInt::from(v); }
    ((acc % p) + p) % p
}

fn fit(r: &[BigInt], honest: &[u8], target: &[u8], p: &BigInt) -> Vec<u8> {
    for attempt in 0..ATTEMPTS {
        let pos = corrective_pixels(attempt);
        let mut ch = target.to_vec();
        for &i in &pos { ch[i] = 128; }
        let gap = ((claim(r, honest, p) - claim(r, &ch, p)) % p + p) % p;
        let coeffs: Vec<BigInt> = pos.iter().map(|&i| r[i].clone()).collect();
        let Some(deltas) = lattice::solve_congruence(&coeffs, &gap, p) else { continue };
        let mut ok = true;
        for (&i, d) in pos.iter().zip(&deltas) {
            match u8::try_from(BigInt::from(128) + d) { Ok(v) => ch[i] = v, Err(_) => { ok = false; break } }
        }
        if ok {
            assert_eq!(claim(r, &ch, p), claim(r, honest, p));
            let max = deltas.iter().map(|d| d.magnitude().clone()).max().unwrap();
            eprintln!("attempt {attempt}: largest |delta| = {max}");
            return ch;
        }
    }
    panic!("no correction within 0..=255");
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (chal, honest, target, out) = (&a[1], &a[2], &a[3], &a[4]);
    // BLS12-381 scalar field modulus.
    let p = BigInt::parse_bytes(b"73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001", 16).unwrap();
    let r: Vec<BigInt> = std::fs::read_to_string(chal).unwrap().lines()
        .map(|l| BigInt::from_bytes_be(Sign::Plus, &(0..l.len()).step_by(2).map(|i| u8::from_str_radix(&l[i..i + 2], 16).unwrap()).collect::<Vec<_>>()))
        .collect();
    let h: Image = serde_json::from_str(&std::fs::read_to_string(honest).unwrap()).unwrap();
    let t: Image = serde_json::from_str(&std::fs::read_to_string(target).unwrap()).unwrap();
    assert_eq!(r.len(), h.R.len());
    let forged = Image {
        rows: h.rows, cols: h.cols,
        R: fit(&r, &h.R, &t.R, &p), G: fit(&r, &h.G, &t.G, &p), B: fit(&r, &h.B, &t.B, &p),
    };
    let changed: usize = [(&h.R, &forged.R), (&h.G, &forged.G), (&h.B, &forged.B)]
        .iter().map(|(a, b)| a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()).sum();
    eprintln!("channel values changed: {changed} of {}", 3 * h.R.len());
    std::fs::write(out, serde_json::to_string(&forged).unwrap()).unwrap();
}
