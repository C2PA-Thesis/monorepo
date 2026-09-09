export type Channel = "R" | "G" | "B";
export type Status =
  | "draft"
  | "queued"
  | "running"
  | "complete"
  | "failed"
  | "cancelled";
export type Stage = {
  id: string;
  number: number;
  title: string;
  description: string;
  status: string;
  seconds: number | null;
  started_at?: string;
};
export type Location = {
  latitude: number;
  longitude: number;
  resolution: number;
  cell?: string;
};
export type Region = {
  cell: string;
  resolution: number;
  boundary: [number, number][];
  center: [number, number];
  area_km2: number;
  contains_point: boolean;
  supported: boolean;
  reason: string | null;
  region?: { face: number; i: number; j: number; k: number };
};
export type Attack = {
  case: string;
  expected: number;
  observed: number;
  c2pa: string;
  passed: boolean;
  time: string;
};
export type Experiment = {
  id: string;
  name: string;
  source: { width: number; height: number; format: string; bytes: number };
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  status: Status;
  activity: string;
  config: (Location & { region: Region }) | null;
  stages: Stage[];
  error: string | null;
  seconds: number;
  attacks: Record<string, Attack>;
};
export type LabEvent = {
  id: number;
  type: string;
  time: string;
  message?: string;
  stage?: string;
  status?: string;
  title?: string;
  seconds?: number;
  result?: Attack;
};
export type Receipt = {
  captured_at: string;
  device_key_id: string;
  signature: string;
  rows: number;
  cols: number;
  envelope: { value: string };
  fingerprint: { field: string; channels: Record<Channel, string[]> };
};
export type Reader = {
  activity: string;
  started_at: string | null;
  error: string | null;
  id: string;
  published: boolean;
  status: Status;
  assertion: {
    receipt: Receipt;
    region: { cell: string; resolution: number };
    location_proof: unknown;
    image_proof: unknown;
  } | null;
  manifest: unknown;
  image_proof: unknown;
  sizes: {
    image_proof: number | null;
    location_proof: number | null;
    signed_png: number | null;
  };
  receipt_benchmark: {
    signature_only_median_seconds: number;
    complete_receipt_check_median_seconds: number;
  } | null;
  negative_results: {
    cases: {
      name: string;
      expected: string;
      observed: string;
      c2pa_validation_state: string;
    }[];
  } | null;
  attacks: Record<string, Attack>;
  reader_check: { time: string; seconds: number; passed: boolean } | null;
  stages: Stage[];
};
export type MatrixData = {
  channel: Channel;
  x: number;
  y: number;
  pixels: [number, number, number][];
  values: number[];
  indices: number[];
  fingerprint: string[] | null;
  field: string;
  input_length: number;
  output_length: number;
};

export const DEFAULT_LOCATION: Location = {
  latitude: -34.5478,
  longitude: -58.4462,
  resolution: 7,
};
export const STAGES = [
  "Capture",
  "Crop",
  "Location proof",
  "Image proof",
  "C2PA package",
  "Reader check",
  "Benchmark",
  "Tamper tests",
];
export const busy = (status?: Status) =>
  status === "queued" || status === "running";
export const asset = (id: string, name: string, download = false) =>
  `/api/runs/${id}/assets/${name}${download ? "?download=true" : ""}`;
export const bytes = (n: number | null | undefined) =>
  n == null ? "—" : n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`;
export const seconds = (n: number) =>
  n >= 60
    ? `${Math.floor(n / 60)}m ${Math.floor(n % 60)}s`
    : `${n.toFixed(1)}s`;
