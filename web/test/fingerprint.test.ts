import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { expect, test } from "vitest";

// The same computation the worker does, on the wasm's built-in test image,
// against the digest pinned in crates/fingerprint/src/lib.rs.
test("the wasm fingerprint of the test image matches the native digest", async () => {
  const bytes = readFileSync(new URL("../public/fingerprint.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const wasm = instance.exports as {
    memory: WebAssembly.Memory;
    alloc(len: number): number;
    fill_test_channel(channel: number, out: number): void;
    hash_rows(pixels: number, rowStart: number, rowEnd: number, out: number): void;
  };
  const PIXELS = 1024 * 512;
  const output = wasm.alloc(3 * 128 * 32);
  for (let channel = 0; channel < 3; channel++) {
    const pixels = wasm.alloc(PIXELS);
    wasm.fill_test_channel(channel, pixels);
    wasm.hash_rows(pixels, 0, 128, output + channel * 128 * 32);
  }
  const digest = createHash("sha256")
    .update(new Uint8Array(wasm.memory.buffer, output, 3 * 128 * 32))
    .digest("hex");
  expect(digest).toBe("cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083");
}, 120_000);
