// The lattice fingerprint of the 1024x512 original, computed on the phone
// with crates/fingerprint built for wasm, spread over web workers.

import type { HashReply, HashRequest } from "./fingerprint.worker";
import type { Fingerprint } from "./receipt";

export const ALGORITHM = "hyperveritas-ajtai-chacha8-v1";
export const WIDTH = 1024;
export const HEIGHT = 512;
const ROWS = 128;
const ELEMENT_BYTES = 32;

export type Channels = [Uint8Array, Uint8Array, Uint8Array];

export async function fingerprint(
  channels: Channels,
  onProgress: (done: number, total: number) => void,
): Promise<Fingerprint> {
  const wasm = await (await fetch("/fingerprint.wasm")).arrayBuffer();
  const count = Math.min(navigator.hardwareConcurrency || 4, ROWS);
  const perWorker = Math.ceil(ROWS / count);
  const ranges = Array.from({ length: count }, (_, i) => [i * perWorker, Math.min((i + 1) * perWorker, ROWS)])
    .filter(([start, end]) => start! < end!);

  let done = 0;
  const replies = await Promise.all(
    ranges.map(async ([rowStart, rowEnd]) => {
      const reply = await hashInWorker(wasm, { channels, rowStart: rowStart!, rowEnd: rowEnd! });
      onProgress(++done, ranges.length);
      return { rowStart: rowStart!, reply };
    }),
  );

  const hex = ([0, 1, 2] as const).map((channel) => {
    const bytes = new Uint8Array(ROWS * ELEMENT_BYTES);
    for (const { rowStart, reply } of replies) bytes.set(reply.rows[channel], rowStart * ELEMENT_BYTES);
    return Array.from({ length: ROWS }, (_, row) => elementHex(bytes.subarray(row * ELEMENT_BYTES, (row + 1) * ELEMENT_BYTES)));
  });
  return { algorithm: ALGORITHM, size: { width: WIDTH, height: HEIGHT }, R: hex[0]!, G: hex[1]!, B: hex[2]! };
}

function hashInWorker(wasm: ArrayBuffer, request: HashRequest): Promise<HashReply> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./fingerprint.worker.ts", import.meta.url), { type: "module" });
    worker.onerror = (event) => reject(new Error(event.message));
    worker.onmessage = (event: MessageEvent<"ready" | HashReply>) => {
      if (event.data === "ready") return worker.postMessage(request);
      worker.terminate();
      resolve(event.data);
    };
    worker.postMessage({ wasm });
  });
}

function elementHex(bytes: Uint8Array): string {
  return `0x${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}
