// The receipt as crates/provenance/src/receipt.rs defines it. The signed
// bytes are the compact JSON of the first four fields in this order, so the
// object literal below must keep the Rust field order.

import { sign, type DeviceKey } from "./key";

export interface Fingerprint {
  algorithm: string;
  size: { width: number; height: number };
  R: string[];
  G: string[];
  B: string[];
}

export interface Receipt {
  fingerprint: Fingerprint;
  envelope: string;
  captured_at: string;
  device: string;
  signature: string;
}

export function signedBytes(receipt: Omit<Receipt, "signature">): Uint8Array<ArrayBuffer> {
  const { fingerprint, envelope, captured_at, device } = receipt;
  return new TextEncoder().encode(JSON.stringify({ fingerprint, envelope, captured_at, device }));
}

export async function signReceipt(
  key: DeviceKey,
  fingerprint: Fingerprint,
  envelope: string,
  capturedAt: string,
): Promise<Receipt> {
  const unsigned = { fingerprint, envelope, captured_at: capturedAt, device: key.id };
  const signature = await sign(key, signedBytes(unsigned));
  return { ...unsigned, signature: btoa(String.fromCharCode(...signature)) };
}

/// RFC 3339 in UTC with whole seconds, as the Rust side formats it.
export function timestamp(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}
