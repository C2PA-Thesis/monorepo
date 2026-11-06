// The calls `provenance serve` answers. See crates/provenance/src/serve.rs.

import type { Rect, Turn } from "./crop";
import type { Receipt } from "./receipt";

export interface Secrets {
  latitude: number;
  longitude: number;
  salt: string;
  /// The eight float32 bit patterns sealed in the envelope next to the coordinate.
  hints: number[];
}

export interface Region {
  cell: string;
  resolution: number;
}

export interface Outline {
  cell: string;
  resolution: number;
  area_km2: number;
  /// Latitude, longitude pairs in degrees.
  boundary: [number, number][];
}

export interface Upload {
  original_png: string;
  secrets: Secrets;
  receipt: Receipt;
  cell: string;
  resolution: number;
  crop: Rect;
  /// Quarter turns clockwise after the crop. The server reads a missing one as none.
  turn: Turn;
  accuracy_meters: number | null;
}

export interface Accepted {
  id: string;
  dir: string;
}

export type Phase =
  | "stored"
  | "published"
  | "queued"
  | "publishing"
  | "verifying"
  | "accepted"
  | "rejected"
  | "failed";

export type Stage = "capture" | "crop" | "location-proof" | "crop-proof" | "publish" | "verify";
export type Check = "manifest" | "device" | "image" | "location";

/// Publish stages and reader checks, as `provenance --json` prints them.
export type Event =
  | { event: "started"; stage: Stage }
  | { event: "finished"; stage: Stage; detail: string; seconds: number }
  | { event: "checking"; check: Check; intent: string }
  | { event: "passed"; check: Check; intent: string; outcome: string; seconds: number }
  | { event: "rejected"; check: Check; intent: string; reason: string; seconds: number };

export interface Claim {
  device: string;
  captured_at: string;
  fingerprint: string;
  envelope: string;
  cell: string;
  crop: Rect;
  /// Absent when there is no turn.
  turn?: Turn;
}

export interface Status {
  id: string;
  phase: Phase;
  device: string;
  device_kind: DeviceKind | null;
  /// The App Attest environment, when the key paired with an attestation that verified.
  device_attested: "development" | "production" | null;
  captured_at: string;
  /// The original's size, from the signed fingerprint.
  original: { width: number; height: number };
  capture: {
    source: "phone" | "simulated";
    cell: string;
    resolution: number;
    crop: Rect;
    /// Absent when there is no turn.
    turn?: Turn;
    accuracy_meters: number | null;
    /// What the app's location service said about its fix. Absent for the capture page.
    location?: { simulated: boolean; accessory: boolean };
  };
  events: Event[];
  error: string | null;
  claim: Claim | null;
  published: boolean;
}

export interface Report {
  events: Event[];
  verdict: {
    passed: { check: Check; intent: string; detail: string }[];
    rejected: { check: Check; intent: string; detail: string } | null;
    claim: Claim | null;
  };
}

export interface Session {
  pairing_code: string;
  public_url: string | null;
  pair_url: string | null;
  publishes: boolean;
  captures: string;
}

export type DeviceKind = "secure-enclave" | "software" | "browser" | "laptop";

/// What the app's QR code holds, encoded as JSON as is.
export interface AppQr {
  v: 1;
  /// The TLS listener, one https URL per network address of the laptop.
  urls: string[];
  /// SHA-256 of the TLS key's SubjectPublicKeyInfo, hex.
  pin: string;
  /// Single use, short lived.
  token: string;
  /// SHA-256 of each prover file at /api/params/location/<name>.
  params: { "proving.key": string; "circuit.r1cs": string };
}

/// `GET /api/v2/pairing`: the payload as the server wrote it, which the QR
/// code holds byte for byte, and how long its token has left.
export interface AppPairing {
  text: string;
  qr: AppQr;
  /// Seconds until the token expires and is replaced.
  expiresIn: number;
}

export const isRunning = (phase: Phase) => phase === "queued" || phase === "publishing" || phase === "verifying";

export function pair(code: string, publicKey: string): Promise<{ device: string }> {
  return post("/api/pair", { code, public_key: publicKey });
}

export async function isTrusted(device: string): Promise<boolean> {
  return (await get<{ trusted: boolean }>(`/api/devices/${device}`)).trusted;
}

export function cell(latitude: number, longitude: number, resolution: number): Promise<Region> {
  return post("/api/cell", { latitude, longitude, resolution });
}

export function outline(cell: string): Promise<Outline> {
  return get(`/api/cells/${cell}`);
}

export function upload(body: Upload): Promise<Accepted> {
  return post("/api/captures", body);
}

export function status(id: string): Promise<Status> {
  return get(`/api/captures/${id}`);
}

export function captures(): Promise<Status[]> {
  return get("/api/captures");
}

export function republish(id: string): Promise<Status> {
  return post(`/api/captures/${id}/publish`, {});
}

export function session(): Promise<Session> {
  return get("/api/session");
}

/// Only answers on the laptop. An expired token is replaced on the way.
export async function appPairing(): Promise<AppPairing> {
  const path = "/api/v2/pairing";
  const response = await fetch(path, { headers: HEADERS, cache: "no-store" });
  const text = await response.text();
  if (!response.ok) {
    let error: string | undefined;
    try {
      error = (JSON.parse(text) as { error?: string }).error;
    } catch {
      // Not JSON: fall through to the status.
    }
    throw new Error(error ?? `${path} failed with ${response.status}`);
  }
  return { text, qr: JSON.parse(text) as AppQr, expiresIn: Number(response.headers.get("x-expires-in") ?? 0) };
}

export const signedUrl = (id: string) => `/api/captures/${id}/signed.png`;
export const originalUrl = (id: string) => `/api/captures/${id}/original.png`;

export async function verify(png: Blob): Promise<Report> {
  return parse(await fetch("/api/verify", { method: "POST", headers: HEADERS, body: png }), "/api/verify");
}

// ngrok's free plan answers a browser's first request with a warning page
// unless this header is present.
const HEADERS = { "ngrok-skip-browser-warning": "1" };

async function get<T>(path: string): Promise<T> {
  return parse(await fetch(path, { headers: HEADERS }), path);
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    headers: { ...HEADERS, "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  return parse(response, path);
}

async function parse<T>(response: Response, path: string): Promise<T> {
  const json = (await response.json().catch(() => ({}))) as T & { error?: string };
  if (!response.ok) throw new Error(json.error ?? `${path} failed with ${response.status}`);
  return json;
}
