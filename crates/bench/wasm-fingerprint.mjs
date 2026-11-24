// Hashes three channels with the module the capture page loads, on one thread.
// Usage: node wasm-fingerprint.mjs MODULE PLANES, where PLANES is R, G, B
// back to back. The hash rows match crates/fingerprint/src/lib.rs.
import { readFileSync } from "node:fs";

const HASH_ROWS = 128;
const ELEMENT_BYTES = 32;

const [module, planesPath] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(readFileSync(module), {});
const wasm = instance.exports;
const planes = readFileSync(planesPath);
const pixels = planes.length / 3;

const inputs = [0, 1, 2].map((channel) => {
  const input = wasm.alloc(pixels);
  new Uint8Array(wasm.memory.buffer, input, pixels).set(planes.subarray(channel * pixels, (channel + 1) * pixels));
  return input;
});
const size = HASH_ROWS * ELEMENT_BYTES;
const outputs = inputs.map(() => wasm.alloc(size));

const started = performance.now();
inputs.forEach((input, channel) => wasm.hash_rows(input, pixels, 0, HASH_ROWS, outputs[channel]));
const seconds = (performance.now() - started) / 1000;

const elements = outputs
  .map((output) => Buffer.from(wasm.memory.buffer, output, size).toString("hex"))
  .join("");
console.log(JSON.stringify({ seconds, elements }));
