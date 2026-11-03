// The app's two marks as SVG: the viewfinder (four corner brackets around the
// hexagon a photo shares, ios/ProvenanceCapture/Views/Components.swift,
// ViewfinderMark), and the seal behind "Verified".

import { h } from "./dom";

const BRACKETS = "M14 32V20a6 6 0 0 1 6-6h12M68 14h12a6 6 0 0 1 6 6v12M86 68v12a6 6 0 0 1-6 6H68M32 86H20a6 6 0 0 1-6-6V68";
const HEXAGON = "M50 30 67.3 40v20L50 70 32.7 60V40z";

/// The viewfinder. Its colours come from the page tokens, so it follows dark mode.
export function viewfinder(label?: string): string {
  const a11y = label ? `role="img" aria-label="${label}"` : `aria-hidden="true"`;
  return `<svg class="viewfinder" viewBox="0 0 100 100" ${a11y}><path class="vf-brackets" d="${BRACKETS}" fill="none" stroke="var(--ink)" stroke-width="8.5" stroke-linecap="round" stroke-linejoin="round"/><path class="vf-hexagon" d="${HEXAGON}" fill="var(--signal)" stroke="var(--signal)" stroke-width="5" stroke-linejoin="round"/></svg>`;
}

/// The viewfinder while something runs: the brackets breathe and the hexagon blinks.
export function loader(size: number, label = "Working"): HTMLElement {
  const element = h("span", { class: "vf-loader", role: "img", "aria-label": label, style: `width:${size}px;height:${size}px` });
  element.innerHTML = viewfinder();
  return element;
}

/// The seal behind "Verified": eight rounded points.
export const seal = (() => {
  const points = Array.from({ length: 16 }, (_, i) => {
    const angle = -Math.PI / 2 + (i * Math.PI) / 8;
    const radius = i % 2 ? 38.5 : 44;
    return `${(50 + radius * Math.cos(angle)).toFixed(2)},${(50 + radius * Math.sin(angle)).toFixed(2)}`;
  }).join(" ");
  return `<svg class="seal" viewBox="0 0 100 100" aria-hidden="true"><polygon points="${points}" fill="currentColor" stroke="currentColor" stroke-width="9" stroke-linejoin="round"/></svg>`;
})();
