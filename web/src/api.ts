// The three calls `provenance serve` answers. See crates/provenance/src/serve.rs.

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

export function pair(code: string, publicKey: string): Promise<{ device: string }> {
  return post("/api/pair", { code, public_key: publicKey });
}

export function cell(latitude: number, longitude: number, resolution: number): Promise<Region> {
  return post("/api/cell", { latitude, longitude, resolution });
}

export function upload(body: Upload): Promise<Accepted> {
  return post("/api/captures", body);
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const response = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const json = (await response.json()) as T & { error?: string };
  if (!response.ok) throw new Error(json.error ?? `${path} failed with ${response.status}`);
  return json;
}
