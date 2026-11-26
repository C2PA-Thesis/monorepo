//! The commitment key as the prover reads it: one Lagrange basis per number
//! of variables.
//!
//! The key file is what `MultilinearProverParam::serialize_uncompressed`
//! writes, with every integer a little-endian u64:
//!
//! ```text
//! num_vars
//! number of bases, num_vars + 1
//! for n = num_vars down to 0:  2^n, then 2^n G1 points   (the basis for n variables)
//! g, a G1 point
//! h, a G2 point of 192 bytes
//! ```
//!
//! A G1 point is 96 bytes: x then y, 48 bytes each, big endian. The top bits
//! of the first byte are flags: 0x80 for a compressed point, which this file
//! never holds, and 0x40 for the point at infinity.

use std::{borrow::Cow, fs::File, ops::Range, path::Path};

use anyhow::{ensure, Context, Result};
use ark_bls12_381::{Bls12_381, G1Affine, G2Affine};
use ark_serialize::{CanonicalDeserialize, SerializationError};
use memmap2::Mmap;
use rayon::prelude::*;
use subroutines::pcs::prelude::MultilinearProverParam;

use crate::iop::check;

/// Bytes of one G1 point in the key file.
const POINT_BYTES: usize = 96;

/// Points one task decodes at least, so a short range is not split further.
const DECODE_TASK: usize = 1 << 10;

// A process holds one key, so the size of the larger variant costs nothing.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Srs {
    /// Deserialized into memory.
    Loaded(MultilinearProverParam<Bls12_381>),
    /// The key file itself, decoded range by range as it is read.
    Mapped(Mapped),
}

/// A key file mapped read-only. Its pages are backed by the file, so the
/// system can drop and reload them, and they do not count as memory the
/// process owns.
pub(crate) struct Mapped {
    map: Mmap,
    /// Where the points of the basis for n variables are in the file.
    bases: Vec<Range<usize>>,
}

impl Srs {
    /// Maps the key file at `path` without reading it into the heap, and
    /// refuses a file that `deserialize_uncompressed_unchecked` would refuse
    /// or whose bases do not have 2^n points.
    ///
    /// Like that loader, it does not check that the points are on the curve
    /// or are the right ones. The caller compares the digest of `bytes()`
    /// with the one setup recorded before it proves with the key.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: the mapping is only sound while nobody changes the file. The
        // key is written once by setup and only read afterwards; rewriting or
        // truncating it while a prover has it mapped is not supported.
        let map =
            unsafe { Mmap::map(&file) }.with_context(|| format!("mapping {}", path.display()))?;
        let bases =
            layout(&map).with_context(|| format!("{} is not a prover key", path.display()))?;
        Ok(Srs::Mapped(Mapped { map, bases }))
    }

    /// The whole key file if it is mapped, to check its digest in place.
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match self {
            Srs::Loaded(_) => None,
            Srs::Mapped(mapped) => Some(&mapped.map),
        }
    }

    /// The largest number of variables a polynomial may have.
    pub(crate) fn num_vars(&self) -> usize {
        match self {
            Srs::Loaded(params) => params.num_vars,
            Srs::Mapped(mapped) => mapped.bases.len() - 1,
        }
    }

    /// The `2^num_vars` points that commit to a polynomial over `num_vars`
    /// variables. Opening a polynomial over n variables uses the bases for
    /// n - 1 down to 0 variables, one per round.
    pub(crate) fn basis(&self, num_vars: usize) -> Basis<'_> {
        assert!(num_vars <= self.num_vars(), "no basis this large");
        match self {
            Srs::Loaded(params) => {
                Basis::Points(&params.powers_of_g[params.num_vars - num_vars].evals)
            }
            Srs::Mapped(mapped) => Basis::Mapped(&mapped.map[mapped.bases[num_vars].clone()]),
        }
    }
}

/// Checks the whole file and returns where each basis is, indexed by its
/// number of variables.
fn layout(bytes: &[u8]) -> Result<Vec<Range<usize>>> {
    let mut rest = bytes;
    let num_vars = read_u64(&mut rest)?;
    ensure!(num_vars < 64, "a key over {num_vars} variables");
    let count = read_u64(&mut rest)?;
    ensure!(
        count == num_vars + 1,
        "{count} bases for {num_vars} variables"
    );
    let mut bases = Vec::with_capacity(count as usize);
    for n in (0..=num_vars).rev() {
        let len = read_u64(&mut rest)?;
        ensure!(
            len == 1 << n,
            "the basis for {n} variables has {len} points"
        );
        let size = usize::try_from(len)
            .ok()
            .and_then(|len| len.checked_mul(POINT_BYTES))
            .context("the file is truncated")?;
        let start = bytes.len() - rest.len();
        let points = take(&mut rest, size)?;
        // After this pass no point of the file can fail to decode.
        check(
            points
                .par_chunks_exact(POINT_BYTES)
                .with_min_len(DECODE_TASK)
                .try_for_each(|point| decode(point).map(drop)),
            "reading a point",
        )?;
        bases.push(start..start + size);
    }
    bases.reverse();
    // The prover never uses g and h, but a key without them is not a key.
    let _: G1Affine = check(decode(take(&mut rest, POINT_BYTES)?), "reading g")?;
    let _: G2Affine = check(
        G2Affine::deserialize_uncompressed_unchecked(take(&mut rest, 2 * POINT_BYTES)?),
        "reading h",
    )?;
    ensure!(
        rest.is_empty(),
        "{} bytes follow the end of the key",
        rest.len()
    );
    Ok(bases)
}

/// Splits `len` bytes off the front of `rest`.
fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    let (taken, after) = rest
        .split_at_checked(len)
        .context("the file is truncated")?;
    *rest = after;
    Ok(taken)
}

fn read_u64(rest: &mut &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(take(rest, 8)?.try_into()?))
}

/// One point of the key file, through the reader arkworks deserializes the
/// key with. It converts both coordinates to Montgomery form, which is the
/// whole cost of reading a mapped key.
fn decode(point: &[u8]) -> Result<G1Affine, SerializationError> {
    G1Affine::deserialize_uncompressed_unchecked(point)
}

/// A point `open` has already decoded once. It can only fail again if the
/// file changed under the mapping, which the mapping does not allow.
fn decode_checked(point: &[u8]) -> G1Affine {
    decode(point).unwrap_or_default()
}

#[derive(Clone, Copy)]
pub(crate) enum Basis<'a> {
    Points(&'a [G1Affine]),
    /// The points as the key file stores them.
    Mapped(&'a [u8]),
}

impl<'a> Basis<'a> {
    pub(crate) fn len(&self) -> usize {
        match self {
            Basis::Points(points) => points.len(),
            Basis::Mapped(bytes) => bytes.len() / POINT_BYTES,
        }
    }

    /// The points of `range`, ready for arithmetic. A mapped basis decodes
    /// them into a new vector of 104 bytes per point, so ask for a range at
    /// a time and never for a whole large basis: 2^20 points are 109 MB.
    pub(crate) fn points(&self, range: Range<usize>) -> Cow<'a, [G1Affine]> {
        match self {
            Basis::Points(points) => Cow::Borrowed(&points[range]),
            Basis::Mapped(_) => {
                let mut decoded = Vec::new();
                self.read(range, &mut decoded);
                Cow::Owned(decoded)
            }
        }
    }

    /// The points of `range`, decoded into `buffer` if they are mapped. A
    /// caller that reads range after range keeps one buffer for all of them.
    pub(crate) fn read<'b>(
        &self,
        range: Range<usize>,
        buffer: &'b mut Vec<G1Affine>,
    ) -> &'b [G1Affine]
    where
        'a: 'b,
    {
        match self {
            Basis::Points(points) => &points[range],
            Basis::Mapped(bytes) => {
                buffer.clear();
                buffer.par_extend(
                    bytes[range.start * POINT_BYTES..range.end * POINT_BYTES]
                        .par_chunks_exact(POINT_BYTES)
                        .with_min_len(DECODE_TASK)
                        .map(decode_checked),
                );
                buffer
            }
        }
    }

    /// One point, decoded if it is mapped.
    pub(crate) fn point(&self, index: usize) -> Cow<'a, G1Affine> {
        match self {
            Basis::Points(points) => Cow::Borrowed(&points[index]),
            Basis::Mapped(bytes) => {
                let start = index * POINT_BYTES;
                Cow::Owned(decode_checked(&bytes[start..start + POINT_BYTES]))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use ark_ec::AffineRepr;
    use ark_serialize::CanonicalSerialize;
    use rand::SeedableRng;
    use subroutines::pcs::prelude::PolynomialCommitmentScheme;
    use tempfile::NamedTempFile;

    use super::*;
    use crate::Pcs;

    /// A key over `num_vars` variables from a fixed seed.
    pub(crate) fn params(num_vars: usize) -> MultilinearProverParam<Bls12_381> {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let srs = Pcs::gen_srs_for_testing(&mut rng, num_vars).unwrap();
        Pcs::trim(&srs, None, Some(num_vars)).unwrap().0
    }

    fn serialized(params: &MultilinearProverParam<Bls12_381>) -> Vec<u8> {
        let mut bytes = Vec::new();
        params.serialize_uncompressed(&mut bytes).unwrap();
        bytes
    }

    fn file_of(bytes: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        file.flush().unwrap();
        file
    }

    /// `params` written to a file and mapped. The file lives as long as the
    /// returned handle.
    pub(crate) fn mapped(params: &MultilinearProverParam<Bls12_381>) -> (NamedTempFile, Srs) {
        let file = file_of(&serialized(params));
        let srs = Srs::open(file.path()).unwrap();
        (file, srs)
    }

    #[test]
    fn mapped_bases_equal_the_loaded_ones() {
        let mut params = params(6);
        // setup never writes the point at infinity; the reader must still
        // decode it as arkworks does.
        params.powers_of_g[0].evals[5] = G1Affine::zero();
        params.powers_of_g[3].evals[0] = G1Affine::zero();
        let bytes = serialized(&params);
        let (_file, srs) = mapped(&params);
        assert_eq!(srs.bytes(), Some(bytes.as_slice()));

        let reloaded =
            MultilinearProverParam::<Bls12_381>::deserialize_uncompressed_unchecked(&bytes[..])
                .unwrap();
        let loaded = Srs::Loaded(reloaded);
        assert!(loaded.bytes().is_none());
        assert_eq!(srs.num_vars(), 6);
        assert_eq!(loaded.num_vars(), 6);
        for num_vars in 0..=6 {
            let expected = &params.powers_of_g[6 - num_vars].evals;
            for srs in [&srs, &loaded] {
                let basis = srs.basis(num_vars);
                assert_eq!(basis.len(), 1 << num_vars);
                assert_eq!(basis.points(0..basis.len()).as_ref(), &expected[..]);
                let mut buffer = Vec::new();
                for (index, point) in expected.iter().enumerate() {
                    assert_eq!(basis.point(index).as_ref(), point);
                    assert_eq!(
                        basis.read(index..basis.len(), &mut buffer),
                        &expected[index..]
                    );
                }
                let middle = basis.len() / 4..basis.len() / 2;
                assert_eq!(basis.points(middle.clone()).as_ref(), &expected[middle]);
            }
        }
    }

    #[test]
    fn malformed_files_are_refused() {
        let good = serialized(&params(6));
        assert!(Srs::open(file_of(&good).path()).is_ok());
        let level = 16 + 8;
        let second_level = level + 64 * POINT_BYTES + 8;
        let change = |at: usize, value: u8| {
            let mut bytes = good.clone();
            bytes[at] = value;
            bytes
        };
        let padded = [good.as_slice(), &[0]].concat();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("header only", good[..16].to_vec()),
            ("one byte short", good[..good.len() - 1].to_vec()),
            ("one point short", good[..good.len() - POINT_BYTES].to_vec()),
            ("no h", good[..good.len() - 2 * POINT_BYTES].to_vec()),
            ("one byte more", padded),
            ("more variables than bases", change(0, 7)),
            ("fewer variables than bases", change(0, 5)),
            ("more bases than variables", change(8, 8)),
            ("a huge number of variables", change(7, 0x80)),
            ("a basis of the wrong size", change(16, 63)),
            (
                "a second basis of the wrong size",
                change(second_level - 8, 33),
            ),
            ("a compressed point", change(level, good[level] | 0x80)),
            ("an x above the modulus", change(level, 0x1f)),
            ("a y above the modulus", change(second_level + 48, 0xff)),
            ("a compressed g", {
                let g = good.len() - 3 * POINT_BYTES;
                change(g, good[g] | 0x80)
            }),
            ("a compressed h", {
                let h = good.len() - 2 * POINT_BYTES;
                change(h, good[h] | 0x80)
            }),
        ];
        for (name, bytes) in cases {
            assert!(Srs::open(file_of(&bytes).path()).is_err(), "{name}");
        }
        assert!(Srs::open(Path::new("/nonexistent/prover.bin")).is_err());
    }

    #[test]
    fn a_flagged_infinity_ignores_its_coordinates() {
        // arkworks reads the infinity flag before the coordinates, so garbage
        // after it is still the point at infinity.
        let mut bytes = serialized(&params(2));
        let point = 16 + 8;
        bytes[point] = 0x40 | 0x1f;
        bytes[point + 60] = 0xff;
        let srs = Srs::open(file_of(&bytes).path()).unwrap();
        let reference =
            MultilinearProverParam::<Bls12_381>::deserialize_uncompressed_unchecked(&bytes[..])
                .unwrap();
        assert!(reference.powers_of_g[0].evals[0].infinity);
        assert_eq!(
            srs.basis(2).points(0..4).as_ref(),
            &reference.powers_of_g[0].evals[..]
        );
    }
}
