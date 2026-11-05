import { expect, test } from "vitest";

import { derSignature } from "../src/key";
import { signedBytes, timestamp } from "../src/receipt";

test("signed bytes are the compact JSON in the Rust field order", () => {
  const bytes = signedBytes({
    fingerprint: { algorithm: "a", size: { width: 1024, height: 512 }, R: ["0x1"], G: ["0x2"], B: ["0x3"] },
    envelope: "0x4",
    captured_at: "2026-09-16T20:00:00Z",
    device: "d",
  });
  expect(new TextDecoder().decode(bytes)).toBe(
    '{"fingerprint":{"algorithm":"a","size":{"width":1024,"height":512},"R":["0x1"],"G":["0x2"],"B":["0x3"]},"envelope":"0x4","captured_at":"2026-09-16T20:00:00Z","device":"d"}',
  );
});

test("timestamps drop milliseconds", () => {
  expect(timestamp(new Date("2026-09-16T20:00:00.789Z"))).toBe("2026-09-16T20:00:00Z");
});

test("DER integers are minimal and positive", () => {
  const r = new Uint8Array(32);
  r[0] = 0x80;
  const s = new Uint8Array(32);
  s[31] = 0x01;
  expect(Array.from(derSignature(r, s))).toEqual([
    0x30, 0x26, 0x02, 0x21, 0x00, 0x80, ...new Array(31).fill(0), 0x02, 0x01, 0x01,
  ]);
});
