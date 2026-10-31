// The envelope: MiMC over BN254 of the coordinate, the salt and the eight
// trigonometric hints, exactly as `location-proof commit` computes it with
// gnark-crypto's MiMC (110 rounds of x^5, constants from a Keccak-256 chain
// seeded with "seed", in a Miyaguchi-Preneel loop). The coordinate and the
// hints are encoded as the circuit takes them: IEEE-754 bits of float32.

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

export function envelope(
  latitude: number,
  longitude: number,
  salt: string,
  sealed: readonly number[] = hints(latitude, longitude),
): string {
  if (sealed.length !== 8 || !sealed.every((hint) => Number.isInteger(hint) && hint >= 0 && hint <= 0xffffffff)) {
    throw new Error("hints must be 8 unsigned 32-bit integers");
  }
  const blocks = [BigInt(radiansBits(latitude)), BigInt(radiansBits(longitude)), parseScalar(salt), ...sealed.map(BigInt)];
  return toHex(mimc(blocks));
}

/// The circuit's hints, as Go's NativeHints computes them: for the latitude,
/// then the longitude, the float32 bits of tan, 2 sin cos, sin and cos of half
/// the float32 radians, each computed in float64 and rounded once.
export function hints(latitude: number, longitude: number): number[] {
  return [radiansBits(latitude), radiansBits(longitude)].flatMap((bits) => {
    const half = fromBits(bits) * 0.5;
    const gamma = Math.sin(half);
    const delta = Math.cos(half);
    return [Math.tan(half), 2 * gamma * delta, gamma, delta].map(toBits);
  });
}

/// A fresh salt below the modulus: 31 random bytes behind a zero byte.
export function freshSalt(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes.subarray(1));
  return `0x${bytesToHex(bytes)}`;
}

/// float32(degrees * PI / 180) as Go computes it, then its bit pattern.
export function radiansBits(degrees: number): number {
  return toBits((degrees * Math.PI) / 180);
}

/// The bit pattern of a number rounded to float32.
function toBits(value: number): number {
  const view = new DataView(new ArrayBuffer(4));
  view.setFloat32(0, Math.fround(value));
  return view.getUint32(0);
}

function fromBits(bits: number): number {
  const view = new DataView(new ArrayBuffer(4));
  view.setUint32(0, bits);
  return view.getFloat32(0);
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
