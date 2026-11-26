# Writes the upstream fixtures for size 2^19: a random 1024x512 original, its
# left half as the honest crop (the layout hv_crop_pst.rs verifies), and an
# unrelated colour gradient of the crop's size as the forger's target.
import json, random, sys
images, run = sys.argv[1], sys.argv[2]
W, H, CW = 1024, 512, 512
rng = random.Random(7)
orig = {c: [rng.randrange(256) for _ in range(W * H)] for c in "RGB"}
crop = {c: [orig[c][y * W + x] for y in range(H) for x in range(CW)] for c in "RGB"}
grad = {
    "R": [(x * 255) // (CW - 1) for y in range(H) for x in range(CW)],
    "G": [(y * 255) // (H - 1) for y in range(H) for x in range(CW)],
    "B": [((x + y) * 255) // (CW + H - 2) for y in range(H) for x in range(CW)],
}
def dump(path, rows, cols, img):
    with open(path, "w") as f:
        json.dump({"rows": rows, "cols": cols, **img}, f)
dump(f"{images}/Timings19.json", W, H, orig)
dump(f"{images}/Crop19.json", CW, H, crop)
dump(f"{run}/target.json", CW, H, grad)
