// The envelope: MiMC over BN254 of the coordinate and the salt, exactly as
// `location-proof commit` computes it with gnark-crypto's MiMC (110 rounds
// of x^5, constants from a Keccak-256 chain seeded with "seed", in a
// Miyaguchi-Preneel loop). The coordinate is encoded as the circuit takes
// it: the IEEE-754 bits of float32 radians.

import { keccak_256 } from "@noble/hashes/sha3.js";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";

/// The BN254 scalar field modulus.
const P = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
const ROUNDS = 110;

const CONSTANTS: bigint[] = (() => {
  let rnd = keccak_256(keccak_256(new TextEncoder().encode("seed")));
  const constants: bigint[] = [];
  for (let i = 0; i < ROUNDS; i++) {
    constants.push(fromBytes(rnd) % P);
    rnd = keccak_256(rnd);
  }
  return constants;
})();

export function envelope(latitude: number, longitude: number, salt: string): string {
  const blocks = [BigInt(radiansBits(latitude)), BigInt(radiansBits(longitude)), parseScalar(salt)];
  return toHex(mimc(blocks));
}

/// A fresh salt below the modulus: 31 random bytes behind a zero byte.
export function freshSalt(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes.subarray(1));
  return `0x${bytesToHex(bytes)}`;
}

/// float32(degrees * PI / 180) as Go computes it, then its bit pattern.
export function radiansBits(degrees: number): number {
  const view = new DataView(new ArrayBuffer(4));
  view.setFloat32(0, Math.fround((degrees * Math.PI) / 180));
  return view.getUint32(0);
}

function mimc(blocks: bigint[]): bigint {
  let h = 0n;
  for (const m of blocks) {
    const r = encrypt(m, h);
    h = (r + h + m) % P;
  }
  return h;
}

function encrypt(m: bigint, h: bigint): bigint {
  for (const c of CONSTANTS) {
    const t = (m + h + c) % P;
    const t2 = (t * t) % P;
    m = (((t2 * t2) % P) * t) % P;
  }
  return (m + h) % P;
}

function parseScalar(text: string): bigint {
  if (!/^0x[0-9a-f]{64}$/.test(text)) throw new Error(`${text} is not 0x followed by 64 hex digits`);
  const value = BigInt(text);
  if (value >= P) throw new Error(`${text} is not below the BN254 scalar modulus`);
  return value;
}

function fromBytes(bytes: Uint8Array): bigint {
  return BigInt(`0x${bytesToHex(bytes)}`);
}

function toHex(value: bigint): string {
  return `0x${value.toString(16).padStart(64, "0")}`;
}

export { hexToBytes };
