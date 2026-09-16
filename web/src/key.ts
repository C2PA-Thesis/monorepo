// The device key: a non-extractable P-256 key the browser creates once and
// keeps in IndexedDB. It stands in for a hardware key; see the README.

import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";

const DB = "capture";
const STORE = "keys";
const NAME = "stand_in_for_secure_enclave_device_key";

export interface DeviceKey {
  pair: CryptoKeyPair;
  /// SHA-256 of the public key in SubjectPublicKeyInfo DER, as the receipt names it.
  id: string;
  pem: string;
}

export async function loadOrCreateKey(): Promise<DeviceKey> {
  const stored = await read();
  const pair =
    stored ??
    (await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]));
  if (!stored) await write(pair);
  const spki = new Uint8Array(await crypto.subtle.exportKey("spki", pair.publicKey));
  return { pair, id: bytesToHex(sha256(spki)), pem: pem(spki) };
}

/// Signs `bytes` and returns the DER-encoded signature the receipt carries.
export async function sign(key: DeviceKey, bytes: Uint8Array<ArrayBuffer>): Promise<Uint8Array> {
  const raw = new Uint8Array(
    await crypto.subtle.sign({ name: "ECDSA", hash: "SHA-256" }, key.pair.privateKey, bytes),
  );
  return derSignature(raw.subarray(0, 32), raw.subarray(32));
}

/// WebCrypto gives r || s; DER wants a SEQUENCE of two minimal positive INTEGERs.
export function derSignature(r: Uint8Array, s: Uint8Array): Uint8Array {
  const integer = (value: Uint8Array) => {
    let start = 0;
    while (start < value.length - 1 && value[start] === 0) start++;
    const magnitude = value.subarray(start);
    const padded = (magnitude[0] ?? 0) & 0x80 ? [0, ...magnitude] : [...magnitude];
    return [0x02, padded.length, ...padded];
  };
  const body = [...integer(r), ...integer(s)];
  return new Uint8Array([0x30, body.length, ...body]);
}

function pem(spki: Uint8Array): string {
  const base64 = btoa(String.fromCharCode(...spki));
  const lines = base64.match(/.{1,64}/g) ?? [];
  return `-----BEGIN PUBLIC KEY-----\n${lines.join("\n")}\n-----END PUBLIC KEY-----\n`;
}

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function read(): Promise<CryptoKeyPair | undefined> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = db.transaction(STORE).objectStore(STORE).get(NAME);
    request.onsuccess = () => resolve(request.result as CryptoKeyPair | undefined);
    request.onerror = () => reject(request.error);
  });
}

async function write(pair: CryptoKeyPair): Promise<void> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(STORE, "readwrite");
    transaction.objectStore(STORE).put(pair, NAME);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
  });
}
