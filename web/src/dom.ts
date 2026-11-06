// A small element builder, so the pages can describe their markup in code.

type Child = Node | string | false | null | undefined;
type Attributes = Record<string, string | boolean | number | undefined | ((event: Event) => void)>;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attributes: Attributes = {},
  ...children: (Child | Child[])[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [name, value] of Object.entries(attributes)) {
    if (value === undefined || value === false) continue;
    if (typeof value === "function") element.addEventListener(name.slice(2).toLowerCase(), value);
    else if (value === true) element.setAttribute(name, "");
    else element.setAttribute(name, String(value));
  }
  element.append(...children.flat().filter((child): child is Node | string => !!child));
  return element;
}

export const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

/// First and last four hex digits, as the CLI shows identifiers.
export function short(hex: string): string {
  const digits = hex.startsWith("0x") ? hex.slice(2) : hex;
  const prefix = hex.startsWith("0x") ? "0x" : "";
  return digits.length <= 10 ? hex : `${prefix}${digits.slice(0, 4)}…${digits.slice(-4)}`;
}

/// The rectangle in the original's pixels, and the turn applied after it when there is one.
export function rect(crop: { x: number; y: number; width: number; height: number }, turn = 0): string {
  return `${crop.width}×${crop.height} at (${crop.x}, ${crop.y})${turn ? `, turned ${turn * 90}°` : ""}`;
}

export function area(km2: number): string {
  if (km2 >= 100) return `${Math.round(km2).toLocaleString("en")} km²`;
  if (km2 >= 1) return `${km2.toFixed(1)} km²`;
  const m2 = km2 * 1e6;
  return m2 >= 10_000 ? `${(m2 / 10_000).toFixed(1)} ha` : `${Math.round(m2).toLocaleString("en")} m²`;
}

/// An H3 index without its unused trailing digits, as the cell is named on screen.
export function cellShort(cell: string): string {
  const trimmed = cell.replace(/f+$/, "");
  return trimmed.length < cell.length ? `${trimmed}…` : cell;
}

/// The time of day today; the day and time otherwise, with the year only when it is not this year.
export function when(rfc3339: string): string {
  const date = new Date(rfc3339);
  if (Number.isNaN(date.getTime())) return rfc3339;
  const now = new Date();
  const clock = date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (date.toDateString() === now.toDateString()) return clock;
  const year = date.getFullYear() === now.getFullYear() ? undefined : "numeric";
  return `${date.toLocaleDateString(undefined, { month: "short", day: "numeric", year })} · ${clock}`;
}

export function time(rfc3339: string): string {
  const date = new Date(rfc3339);
  return Number.isNaN(date.getTime())
    ? rfc3339
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "medium" });
}

export const icon = {
  check: `<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4.5 10.5l3.5 3.5 7.5-8" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  again: `<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M15.5 10a5.5 5.5 0 1 1-1.6-3.9M15.5 3.5v3h-3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  rotate: `<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3.5" y="9" width="9" height="7.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M8 5.5h3.5a4.5 4.5 0 0 1 4.5 4.5v1.5M14 9.5l2 2 2-2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
  cross: `<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 5.5l9 9m0-9l-9 9" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"/></svg>`,
};

export function svg(markup: string): HTMLElement {
  const span = h("span", { class: "icon" });
  span.innerHTML = markup;
  return span;
}
