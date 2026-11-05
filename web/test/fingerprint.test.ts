import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { expect, test } from "vitest";

import { fit, MAX_PIXELS } from "../src/fingerprint";

interface Wasm {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  max_pixels(): number;
  fill_test_channel(channel: number, len: number, out: number): void;
  hash_rows(pixels: number, len: number, rowStart: number, rowEnd: number, out: number): void;
}

async function load(): Promise<Wasm> {
  const bytes = readFileSync(new URL("../public/fingerprint.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return instance.exports as unknown as Wasm;
}

// The same computation the worker does, on the wasm's built-in test image.
function testImageDigest(wasm: Wasm, pixels: number): string {
  const output = wasm.alloc(3 * 128 * 32);
  for (let channel = 0; channel < 3; channel++) {
    const input = wasm.alloc(pixels);
    wasm.fill_test_channel(channel, pixels, input);
    wasm.hash_rows(input, pixels, 0, 128, output + channel * 128 * 32);
  }
  return createHash("sha256")
    .update(new Uint8Array(wasm.memory.buffer, output, 3 * 128 * 32))
    .digest("hex");
}

// Against the digests pinned in crates/fingerprint/src/lib.rs.
test("the wasm fingerprint of the test images matches the native digests", async () => {
  const wasm = await load();
  expect(wasm.max_pixels()).toBe(MAX_PIXELS);
  expect(testImageDigest(wasm, 1024 * 512)).toBe("cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083");
  expect(testImageDigest(wasm, 836 * 627)).toBe("32ae4e96e4679d0dad3e1b10dde31f46d18d15ac25606f73b0f4a014117ab375");
}, 240_000);

// The values Size::fit gives in crates/fingerprint.
test("fit matches the native sizes", () => {
  expect(fit(4032, 3024)).toEqual({ width: 836, height: 627 });
  expect(fit(3024, 4032)).toEqual({ width: 627, height: 836 });
  expect(fit(8064, 6048)).toEqual({ width: 836, height: 627 });
  expect(fit(1920, 1080)).toEqual({ width: 965, height: 543 });
  expect(fit(1024, 512)).toEqual({ width: 1024, height: 512 });
  expect(fit(640, 480)).toEqual({ width: 640, height: 480 });
});
