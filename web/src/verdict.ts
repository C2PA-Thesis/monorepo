// How an outcome reads to a person: the verdict, the joint statement an
// accepted file supports, and the size of an H3 cell at each resolution.

import type { Claim } from "./api";
import { area, h, icon, rect, short, svg, time } from "./dom";

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
  return `${km2 === undefined ? `about ${area(AVERAGE_KM2[resolution]!)}` : area(km2)}, ${SCALE[resolution]}`;
}

export function accepted(claim: Claim): HTMLElement {
  return h(
    "div",
    { class: "verdict ok" },
    svg(icon.check),
    h(
      "div",
      {},
      h("h2", {}, "Verified"),
      h(
        "p",
        {},
        `These pixels are the ${rect(claim.crop)} of an original that a trusted device signed together with a coordinate inside cell `,
        h("span", { class: "mono" }, claim.cell),
        ".",
      ),
    ),
  );
}

export function rejected(title: string, reason: string): HTMLElement {
  return h("div", { class: "verdict bad" }, svg(icon.cross), h("div", {}, h("h2", {}, title), h("p", {}, reason)));
}

export function facts(claim: Claim): HTMLElement {
  const row = (label: string, value: string | Node) => [h("dt", {}, label), h("dd", {}, value)];
  return h(
    "dl",
    { class: "facts" },
    row("Published", rect(claim.crop)),
    row("Cell", h("span", { class: "mono" }, claim.cell)),
    row("Captured", `${time(claim.captured_at)} (device clock)`),
    row("Device", h("span", { class: "mono" }, short(claim.device))),
    row("Fingerprint", h("span", { class: "mono" }, short(claim.fingerprint))),
    row("Envelope", h("span", { class: "mono" }, short(claim.envelope))),
  );
}

export const CAVEAT =
  "The location proof holds for an honest prover only: whoever holds the coordinate and the salt could prove another cell. See Known limits in the README.";
