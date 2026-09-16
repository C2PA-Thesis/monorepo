/// <reference lib="webworker" />
// One wasm instance per worker; each hashes a range of rows of every channel.

const PIXELS = 1024 * 512;
const ELEMENT_BYTES = 32;

interface Exports {
  memory: WebAssembly.Memory;
  alloc(len: number): number;
  hash_rows(pixels: number, rowStart: number, rowEnd: number, out: number): void;
}

export interface HashRequest {
  channels: [Uint8Array, Uint8Array, Uint8Array];
  rowStart: number;
  rowEnd: number;
}

export type HashReply = { rows: [Uint8Array, Uint8Array, Uint8Array] };

let exports: Exports | undefined;

self.onmessage = async (event: MessageEvent<{ wasm: ArrayBuffer } | HashRequest>) => {
  if ("wasm" in event.data) {
    const { instance } = await WebAssembly.instantiate(event.data.wasm, {});
    exports = instance.exports as unknown as Exports;
    self.postMessage("ready");
    return;
  }
  if (!exports) throw new Error("worker used before its wasm loaded");
  const { channels, rowStart, rowEnd } = event.data;
  const rows = channels.map((pixels) => hash(exports!, pixels, rowStart, rowEnd)) as HashReply["rows"];
  self.postMessage({ rows } satisfies HashReply);
};

function hash(wasm: Exports, pixels: Uint8Array, rowStart: number, rowEnd: number): Uint8Array {
  const input = wasm.alloc(PIXELS);
  new Uint8Array(wasm.memory.buffer, input, PIXELS).set(pixels);
  const size = (rowEnd - rowStart) * ELEMENT_BYTES;
  const output = wasm.alloc(size);
  wasm.hash_rows(input, rowStart, rowEnd, output);
  return new Uint8Array(wasm.memory.buffer, output, size).slice();
}
