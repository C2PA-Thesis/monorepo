// How an outcome reads to a person: the status mark, the verdict, the facts
// an accepted file supports, and the size of an H3 cell at each resolution.

import type { Check, Claim, Event as Progress } from "./api";
import { area, cellShort, h, icon, rect, short, svg, time, when } from "./dom";

/// Average hexagon area in km² per resolution, from h3geo.org, "Tables of
/// Cell Statistics Across Resolutions". A given cell can differ by a factor
/// of two.
export const AVERAGE_KM2 = [
  4357449.416, 609788.442, 86801.78, 12393.435, 1770.348, 252.904, 36.129, 5.161, 0.737, 0.105, 0.015,
  0.00215, 0.000307, 0.0000439, 0.00000627, 0.000000895,
];

const SCALE = [
  "a continent",
  "a subcontinent",
  "a large country",
  "a province",
  "a metropolitan area",
  "a city",
  "a district",
  "a neighbourhood",
  "a few blocks",
  "a city block",
  "a large building",
  "a building",
  "a house",
  "a room",
  "a desk",
  "a floor tile",
];

/// The size of a cell at `resolution`: the average until the actual cell's area is known.
export function resolutionText(resolution: number, km2?: number): string {
  return `${km2 === undefined ? `~${area(AVERAGE_KM2[resolution]!)}` : area(km2)} · ${SCALE[resolution]}`;
}

export type Tone = "ok" | "bad" | "running" | "idle";

/// The status mark: a filled check, a filled cross, a spinner or a faint dot.
export function mark(tone: Tone): HTMLElement {
  if (tone === "ok") return h("span", { class: "mark ok" }, svg(icon.check));
  if (tone === "bad") return h("span", { class: "mark bad" }, svg(icon.cross));
  return h("span", { class: "mark" }, tone === "running" ? h("span", { class: "spinner" }) : h("span", { class: "dot" }));
}

export function accepted(action?: HTMLElement): HTMLElement {
  return h("div", { class: "verdict" }, mark("ok"), h("h2", {}, "Verified"), action ?? false);
}

/// The verdict's line before there is a verdict, so the verdict replaces it in place.
export function pending(tone: "running" | "idle", title: string, action?: HTMLElement): HTMLElement {
  return h("div", { class: "verdict is-pending" }, mark(tone), h("h2", {}, title), action ?? false);
}

/// "Rejected" and one short sentence. The server's own words sit behind the
/// failed step's "Details" in the timeline.
export function rejected(title: string, reason: string, action?: HTMLElement): HTMLElement {
  return h(
    "div",
    { class: "verdict-block" },
    h("div", { class: "verdict" }, mark("bad"), h("h2", {}, title), action ?? false),
    reason ? h("p", { class: "verdict-reason" }, reason) : false,
  );
}

/// What a refused check means, in one sentence.
export function reason(check: Check, detail: string): string {
  switch (check) {
    case "manifest":
      return /jumbf|no manifest|not found/i.test(detail) ? "No C2PA manifest in this file." : "The C2PA manifest is not valid.";
    case "device":
      return "No trusted device signed this capture.";
    case "image":
      return "These pixels are not the signed original's.";
    case "location":
      return "The location proof does not match the cell.";
  }
}

/// The sentence for a capture that stopped: the refused check when there is
/// one, else the outermost part of the error.
export function stopped(events: Progress[], error: string | null): string {
  const refusal = [...events].reverse().find((event) => event.event === "rejected");
  if (refusal?.event === "rejected") return reason(refusal.check, refusal.reason);
  return error ? sentence(tidy(error).split(": ")[0]!) : "";
}

/// Server text without the temporary paths it names.
export function tidy(text: string): string {
  return text.replace(/(?:\/[\w.+~-]+){2,}/g, "the file");
}

const sentence = (text: string) => {
  const trimmed = text.trim();
  return trimmed ? `${trimmed[0]!.toUpperCase()}${trimmed.slice(1)}${/[.!?]$/.test(trimmed) ? "" : "."}` : "";
};

/// Three facts, and the identifiers behind "Details". `km2` is the cell's
/// area once the map has fetched its outline. `key` names the kind of key
/// after its id, and `details` adds rows after it.
export function facts(
  claim: Claim,
  km2?: number,
  options: { key?: string; details?: [string, string][] } = {},
): HTMLElement {
  const row = (label: string, value: string | Node) => [h("dt", {}, label), h("dd", {}, value)];
  return h(
    "div",
    { class: "facts-block" },
    h(
      "dl",
      { class: "facts" },
      row("Published", rect(claim.crop)),
      row(
        "Cell",
        h("span", { title: claim.cell }, h("span", { class: "mono" }, cellShort(claim.cell)), km2 === undefined ? "" : ` · ${area(km2)}`),
      ),
      row("Captured", h("span", { class: "num", title: time(claim.captured_at) }, when(claim.captured_at))),
    ),
    h(
      "details",
      { class: "more" },
      h("summary", {}, "Details"),
      h(
        "dl",
        { class: "facts" },
        row("Key", h("span", {}, h("span", { class: "mono" }, short(claim.device)), options.key ? ` · ${options.key}` : "")),
        (options.details ?? []).flatMap(([label, value]) => row(label, value)),
        row("Fingerprint", h("span", { class: "mono" }, short(claim.fingerprint))),
        row("Envelope", h("span", { class: "mono" }, short(claim.envelope))),
      ),
    ),
  );
}

/// The one honest limit, said once on a result. The README has the long version.
export const CAVEAT = "The cell holds if the device computed the sealed hints from its position.";
