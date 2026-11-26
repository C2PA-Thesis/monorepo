# Builds examples/hv_crop_pst_a3.rs: the patched hv_crop_pst.rs with a
# dishonest prover for A3. Only prover code changes; ver() is untouched.
import sys
src, dst = sys.argv[1], sys.argv[2]
s = open(src).read()

def sub(old, new, count=1):
    global s
    n = s.count(old)
    assert n == count, (n, old[:80])
    s = s.replace(old, new)

# 1. The range check inside hashPreimageIOP uses the forged version.
sub("let (multsetProof, fx, gx, h, poly_infoProd) = range_checkProverIOP::<F, E, PCS>(",
    "let (multsetProof, fx, gx, h, poly_infoProd) = forged_range_check::<F, E, PCS>(")

# 2. After the camera's honest digest, build the forged channels.
sub('''    println!("setup done!\\n");''',
'''    println!("setup done!\\n");

    // A3: the forger knows only the published digest. It publishes a target
    // crop, fills the rest of the original with gray, and solves 128 pixels
    // outside the crop so that A * I' equals the digest exactly.
    let targetFile = std::env::var("HV_PROVER_CROP").unwrap_or(format!("images/Crop{}.json", testSize));
    let targetImg = load_image(&targetFile);
    let forgedEvals = forge_original::<F>(numCols, origImg.rows, &targetImg, &testDigestRGB);''')

# 3. The prover commits to and proves over the forged channels.
sub('''    let origImg = load_image(&fileName);
    let mut polies = Vec::new();
    polies.push(vec_to_poly(toFieldVec(&origImg.R)).0);
    polies.push( vec_to_poly(toFieldVec(&origImg.G)).0);
    polies.push( vec_to_poly(toFieldVec(&origImg.B)).0);''',
'''    let origImg = load_image(&fileName);
    let mut polies = Vec::new();
    polies.push(vec_to_poly(forgedEvals[0].clone()).0);
    polies.push(vec_to_poly(forgedEvals[1].clone()).0);
    polies.push(vec_to_poly(forgedEvals[2].clone()).0);''')
sub('''            RGBEvals,
            &mut transcript,''', '''            forgedEvals.clone(),
            &mut transcript,''')
sub('''    let cropFileName = format!("images/Crop{}.json", testSize);

    let cropImg = load_image(cropFileName);''',
'''    let cropImg = load_image(&targetFile);''')
sub('''    let (origImgRPoly, _) = vec_to_poly(toFieldVec::<F>(&origImg.R));
    let (origImgGPoly, _) = vec_to_poly(toFieldVec::<F>(&origImg.G));
    let (origImgBPoly, _) = vec_to_poly(toFieldVec::<F>(&origImg.B));

    let (cropImgPolyR''', '''    let (origImgRPoly, _) = vec_to_poly(forgedEvals[0].clone());
    let (origImgGPoly, _) = vec_to_poly(forgedEvals[1].clone());
    let (origImgBPoly, _) = vec_to_poly(forgedEvals[2].clone());

    let (cropImgPolyR''')

s += r'''

// A3 forger.

/// Field values for each channel: the target in the crop (left `target.rows`
/// columns of each row), gray elsewhere, and 128 free pixels outside the crop
/// solved so that A * values equals `digest` exactly. The free values are
/// arbitrary field elements, not pixels.
fn forge_original<F: PrimeField>(numCols: usize, width: usize, target: &Image, digest: &Vec<Vec<F>>) -> [Vec<F>; 3] {
    let n = 1usize << numCols;
    let (cw, ch) = (target.rows, target.cols);
    let free: Vec<usize> = (0..n).filter(|&i| i % width >= cw || i / width >= ch).take(128).collect();
    let chans = [&target.R, &target.G, &target.B];
    let mut vals: Vec<Vec<F>> = (0..3).map(|_| vec![F::from(128u64); n]).collect();
    for k in 0..3 {
        for y in 0..ch { for x in 0..cw { vals[k][y * width + x] = F::from(chans[k][y * cw + x] as u64); } }
        for &i in &free { vals[k][i] = F::zero(); }
    }
    // One pass over A: partial digests and the columns at the free pixels.
    let mut partial = vec![vec![F::zero(); 128]; 3];
    let mut cols = vec![vec![F::zero(); 128]; 128];
    for row in 0..128 {
        let mut rng = ChaCha8Rng::seed_from_u64(row as u64);
        let mut f = 0;
        for j in 0..n {
            let a = F::rand(&mut rng);
            for k in 0..3 { partial[k][row] += a * vals[k][j]; }
            if f < 128 && free[f] == j { cols[row][f] = a; f += 1; }
        }
    }
    for k in 0..3 {
        let rhs: Vec<F> = (0..128).map(|r| digest[k][r] - partial[k][r]).collect();
        let sol = solve_square::<F>(cols.clone(), rhs);
        for (m, &i) in free.iter().enumerate() { vals[k][i] = sol[m]; }
        let max_pixel = F::from(255u64).into_bigint();
        let outside = sol.iter().filter(|v| v.into_bigint() > max_pixel).count();
        println!("A3 channel {k}: {outside} of 128 solved values outside 0..255");
    }
    [vals[0].clone(), vals[1].clone(), vals[2].clone()]
}

fn solve_square<F: PrimeField>(mut m: Vec<Vec<F>>, mut b: Vec<F>) -> Vec<F> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).find(|&r| !m[r][c].is_zero()).expect("singular");
        m.swap(c, p); b.swap(c, p);
        let inv = m[c][c].inverse().unwrap();
        for j in c..n { m[c][j] *= inv; }
        b[c] *= inv;
        for r in 0..n {
            if r != c && !m[r][c].is_zero() {
                let f = m[r][c];
                for j in c..n { let v = m[c][j]; m[r][j] -= f * v; }
                let v = b[c]; b[r] -= f * v;
            }
        }
    }
    b
}

/// The honest range check, except that H is chosen after the two challenges,
/// which the verifier draws before it absorbs the commitment to H.
pub fn forged_range_check<F: PrimeField, E, PCS>(
    nv: usize,
    maxVal: u32,
    p1: Arc<DenseMultilinearExtension<F>>,
    primPolyForT: u64,
    primPolyForH: u64,
    transcript: &mut IOPTranscript<E::ScalarField>,
    pcs_param: &PCS::ProverParam,
    ver_param: &PCS::VerifierParam,
) -> (
    <PolyIOP<E::ScalarField> as ProductCheck<E, PCS>>::ProductCheckProof,
    Arc<DenseMultilinearExtension<F>>,
    Arc<DenseMultilinearExtension<F>>,
    Arc<DenseMultilinearExtension<F>>,
    VPAuxInfo<F>
)
where
    E: Pairing<ScalarField = F>,
    PCS: PolynomialCommitmentScheme<
        E,
        Polynomial = Arc<DenseMultilinearExtension<E::ScalarField>>,
        Point = Vec<F>,
        Evaluation = F,
    >,
{
    // The public table and its shift, as in range_checkProverIOP.
    let mut embeddedTable: Vec<F> = vec![F::zero(); 1 << nv];
    let mut plusOneTable: Vec<F> = vec![F::zero(); 1 << nv];
    let galoisRep = (primPolyForT) - (1 << nv);
    let size = 1 << nv;
    let mut binaryString: u64 = 1;
    for i in 1..(maxVal as usize + 1) {
        embeddedTable[binaryString as usize] = F::from(i as u64);
        binaryString <<= 1;
        if (binaryString & size != 0) { binaryString ^= galoisRep; }
        binaryString = (size - 1) & binaryString;
        plusOneTable[binaryString as usize] = F::from(i as u64);
    }
    let polyTable = DenseMultilinearExtension::from_evaluations_vec(nv, embeddedTable);
    let polyPlusOneTable = DenseMultilinearExtension::from_evaluations_vec(nv, plusOneTable);
    let g1 = merge_polynomials::<F>(&[p1.clone(), Arc::new(polyTable)]).unwrap();
    let g2 = merge_polynomials::<F>(&[p1, Arc::new(polyPlusOneTable)]).unwrap();

    // multsetProverIOP draws these two first; the forger reads them ahead.
    let mut peek = transcript.clone();
    let a1 = peek.get_and_append_challenge(b"alpha").unwrap();
    let a2 = peek.get_and_append_challenge(b"alpha").unwrap();
    let target: F = g1.evaluations.iter().zip(&g2.evaluations).map(|(x, y)| *x + a1 + a2 * y).product();

    // H is zero except at slots 4 and 1. Each nonzero slot enters its own
    // factor and the next slot's, so fixing slot 4 leaves a quadratic in slot 1.
    let size: u64 = 1 << (nv + 1);
    let red = primPolyForH - size;
    let times_x = |b: u64| { let mut s = b << 1; if s & size != 0 { s ^= red; } s & (size - 1) };
    let rest = a1.pow([size - 4]);
    for tweak in 1u64..1000 {
        let u = F::from(tweak);
        let k = target / (rest * (u + a1) * (a1 + a2 * u));
        let (a, b, c) = (a2, a1 + a2 * a1, a1 * a1 - k);
        let Some(root) = (b * b - F::from(4u64) * a * c).sqrt() else { continue };
        let t = (root - b) / (a + a);
        let mut h = vec![F::zero(); size as usize];
        h[4] = u;
        h[1] = t;
        let mut sh = vec![F::zero(); size as usize];
        for slot in 1..size { sh[times_x(slot) as usize] = h[slot as usize]; }
        let product: F = h.iter().zip(&sh).map(|(x, y)| *x + a1 + a2 * y).product();
        assert!(product == target, "forged H misses the product");
        let hPoly = Arc::new(DenseMultilinearExtension::from_evaluations_vec(nv + 1, h));
        let shPoly = Arc::new(DenseMultilinearExtension::from_evaluations_vec(nv + 1, sh));
        let (multsetProof, fx, gx, poly_infoProd) = multsetProverIOP::<F, E, PCS>(
            nv + 1, &[g1, g2], &[hPoly.clone(), shPoly], &pcs_param, &ver_param, transcript,
        );
        return (multsetProof, fx, gx, hPoly, poly_infoProd);
    }
    panic!("no square discriminant in 1000 tries");
}
'''
open(dst, "w").write(s)
