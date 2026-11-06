// The lattice fingerprint of the original, computed on the phone with
// crates/fingerprint built for wasm, spread over web workers.

import type { HashReply, HashRequest } from "./fingerprint.worker";
import type { Fingerprint } from "./receipt";

export const ALGORITHM = "hyperveritas-ajtai-chacha8-v1";
/// The most pixels an original may have (crates/fingerprint MAX_PIXELS).
export const MAX_PIXELS = 1 << 19;
const ROWS = 128;
const ELEMENT_BYTES = 32;

export type Channels = [Uint8Array, Uint8Array, Uint8Array];

export interface Size {
  width: number;
  height: number;
}

/// The size of the original for a photo: the photo itself if it fits in
/// MAX_PIXELS, otherwise the largest size with its aspect ratio that does.
/// The same float operations as Size::fit in crates/fingerprint.
export function fit(width: number, height: number): Size {
  if (width * height <= MAX_PIXELS) return { width, height };
  const scale = Math.sqrt(MAX_PIXELS / (width * height));
  const size = { width: Math.max(1, Math.floor(width * scale)), height: Math.max(1, Math.floor(height * scale)) };
  while (size.width * size.height > MAX_PIXELS) {
    if (size.width >= size.height) size.width--;
    else size.height--;
  }
  return size;
}

export async function fingerprint(
  channels: Channels,
  size: Size,
  onProgress: (done: number, total: number) => void,
): Promise<Fingerprint> {
  const pixels = size.width * size.height;
  if (pixels < 1 || pixels > MAX_PIXELS || channels.some((channel) => channel.length !== pixels)) {
    throw new Error(`the planes do not match a ${size.width}x${size.height} original`);
  }
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
  return { algorithm: ALGORITHM, size, R: hex[0]!, G: hex[1]!, B: hex[2]! };
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
