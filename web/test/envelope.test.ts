import { expect, test } from "vitest";

import { envelope, hints, radiansBits } from "../src/envelope";

// Produced by `location-proof hints` and `location-proof commit` on 2026-10-30
// with the sealed-hints zk-Location fork (branch c2p-77-sealed-hints).
const VECTORS = [
  [-34.5478, -58.4462, "0x0000000000000000000000000000000000000000000000000000000000000001", [0xbe9f36e4, 0xbf112d0c, 0xbe980882, 0x3f747413, 0xbf0f3584, 0xbf5a2673, 0xbef9f6de, 0x3f5f6aed], "0x0c5fe416c2846000ee1e623e19158db8cb35dbb69617a5c99ea523839e75ed29"],
  [35.6812, 139.7671, "0x00a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f", [0x3ea4c917, 0x3f15517e, 0x3e9cdc84, 0x3f73b08a, 0x402ebb97, 0x3f255973, 0x3f706203, 0x3eb0177f], "0x2f07c934e38791f372d5f081c79b6ec8a1f264bb0222e9190167ab2497d69ce5"],
  [0, 0, "0x0000000000000000000000000000000000000000000000000000000000000000", [0x00000000, 0x00000000, 0x00000000, 0x3f800000, 0x00000000, 0x00000000, 0x00000000, 0x3f800000], "0x0b60c7d62f2143077c006f3ba330004a524b2e8b058fb6f94294783e4fac4aef"],
  [-90, 180, "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff", [0xbf800000, 0xbf800000, 0xbf3504f3, 0x3f3504f3, 0xcbae8a4a, 0xb3bbbd2e, 0x3f800000, 0xb33bbd2e], "0x2d4c8b07e8aca1f56c735ce7ec66b4e2207d23d79b2e8a0d7f143372a5e6c02e"],
  [90, -180, "0x0000000000000000000000000000000000000000000000000000000000000002", [0x3f800000, 0x3f800000, 0x3f3504f3, 0x3f3504f3, 0x4bae8a4a, 0x33bbbd2e, 0xbf800000, 0xb33bbd2e], "0x186aa8b1a5a28b23f947813e523f996dd9bd5b23c9d6a000beb832b1fe7eb0b1"],
  [89.99999, 179.99999, "0x0000000000000000000000000000000000000000000000000000000000000003", [0x3f7ffffd, 0x3f800000, 0x3f3504f2, 0x3f3504f4, 0x4b4a1bda, 0x34222169, 0x3f800000, 0x33a22169], "0x1d18ade58747e6c2422853e92c37bc42dd1754b065c7eb66eb2ddb8463bb8a8b"],
  [-89.99999, -179.99999, "0x0000000000000000000000000000000000000000000000000000000000000004", [0xbf7ffffd, 0xbf800000, 0xbf3504f2, 0x3f3504f4, 0xcb4a1bda, 0xb4222169, 0xbf800000, 0x33a22169], "0x227d6fbdd254f8c32fc0b0e8c3b90b840578e0930816b3302af1982fb11c44cf"],
  [-33.8688, 151.2093, "0x0000000000000000000000000000000000000000000000000000000000000005", [0xbe9be4b2, 0xbf0eaac2, 0xbe952235, 0x3f74e649, 0x407958f7, 0x3ef695c8, 0x3f77f663, 0x3e7e93f7], "0x18a7d8c9bbee7f8e4c1a0d9984dca1d9593e2a2841d51c3f94f46bd8530853b2"],
  [64.1466, -21.9426, "0x0000000000000000000000000000000000000000000000000000000000000006", [0x3f206c1d, 0x3f6660af, 0x3f07efdd, 0x3f58ed34, 0xbe468360, 0xbebf52aa, 0xbe42e288, 0x3f7b522d], "0x2ac11dda5237166c253d3324e94f4d4ddff98295b893cd5924927cbc370a1e68"],
] as const;

test.each(VECTORS)("hints of (%s, %s) match location-proof hints", (latitude, longitude, _salt, expected) => {
  expect(hints(latitude, longitude)).toEqual(expected);
});

test.each(VECTORS)("envelope of (%s, %s) matches location-proof commit", (latitude, longitude, salt, sealed, expected) => {
  expect(envelope(latitude, longitude, salt)).toBe(expected);
  expect(envelope(latitude, longitude, salt, sealed)).toBe(expected);
});

test("the envelope seals the hints", () => {
  const [latitude, longitude, salt, sealed, expected] = VECTORS[0];
  const other = sealed.map((hint, i) => (i === 2 ? hint + 1 : hint));
  expect(envelope(latitude, longitude, salt, other)).not.toBe(expected);
  expect(() => envelope(latitude, longitude, salt, sealed.slice(0, 7))).toThrow(/8 unsigned/);
});

test("coordinates are encoded as float32 radians", () => {
  expect(radiansBits(180)).toBe(0x40490fdb); // float32(PI)
  expect(radiansBits(0)).toBe(0);
});

test("a salt at or above the modulus is refused", () => {
  expect(() => envelope(0, 0, "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001")).toThrow(/modulus/);
});
