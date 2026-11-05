// The calls `provenance serve` answers. See crates/provenance/src/serve.rs.

import type { Rect } from "./crop";
import type { Receipt } from "./receipt";

export interface Secrets {
  latitude: number;
  longitude: number;
  salt: string;
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
}

export interface Status {
  id: string;
  phase: Phase;
  device: string;
  captured_at: string;
  capture: {
    source: "phone" | "simulated";
    cell: string;
    resolution: number;
    crop: Rect;
    accuracy_meters: number | null;
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
