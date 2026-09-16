import { expect, test } from "vitest";

import { envelope, radiansBits } from "../src/envelope";

// Produced by `location-proof commit` on 2026-09-16 with the pinned zk-Location fork.
const VECTORS = [
  [-34.5478, -58.4462, "0x0000000000000000000000000000000000000000000000000000000000000001", "0x2ff36ff60caf6ee2b95df517e870da3d94e23c24d91bfb751541d0be91c787b1"],
  [35.6812, 139.7671, "0x00a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f", "0x1d625f18bec46357d19c4f26acdf84c1b762f14ec042c73baa054ed200bf7f54"],
  [0, 0, "0x0000000000000000000000000000000000000000000000000000000000000000", "0x2fcb20c300d847eef6fcf6c01760eaa9ff27c3a6d66cd1dc39cc9233cc7460c8"],
  [-90, 180, "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", "0x29b7ab5123c198ecd9b6cc6302d6995a792c1803194d99826901349e757b199b"],
] as const;

test.each(VECTORS)("envelope of (%s, %s) matches location-proof commit", (latitude, longitude, salt, expected) => {
  expect(envelope(latitude, longitude, salt)).toBe(expected);
});

test("coordinates are encoded as float32 radians", () => {
  expect(radiansBits(180)).toBe(0x40490fdb); // float32(PI)
  expect(radiansBits(0)).toBe(0);
});

test("a salt at or above the modulus is refused", () => {
  expect(() => envelope(0, 0, "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001")).toThrow(/modulus/);
});
