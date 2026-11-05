// The rectangle of the original that gets published. Dragging inside it
// moves it; dragging anywhere else draws a new one. Coordinates are in the
// original's pixels.

import type { Size } from "./fingerprint";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/// The left half, rounded down, as Rect::left_half in crates/crop-proof.
export function leftHalf({ width, height }: Size): Rect {
  return { x: 0, y: 0, width: Math.max(1, Math.floor(width / 2)), height };
}

export const PRESETS: { name: string; rect: (size: Size) => Rect }[] = [
  { name: "Left half", rect: leftHalf },
  {
    name: "Center",
    rect: (size) => {
      const half = leftHalf(size);
      return { ...half, x: Math.floor((size.width - half.width) / 2) };
    },
  },
  {
    name: "Right half",
    rect: (size) => {
      const half = leftHalf(size);
      return { ...half, x: size.width - half.width };
    },
  },
  { name: "Whole frame", rect: ({ width, height }) => ({ x: 0, y: 0, width, height }) },
];

const MIN_SIDE = 16;

export interface CropSelector {
  get(): Rect;
  set(rect: Rect): void;
  /// A new original: its size, and the crop back to its left half.
  reset(size: Size): void;
  size(): Size;
}

export function cropSelector(
  stage: HTMLElement,
  box: HTMLElement,
  onChange: (rect: Rect, size: Size) => void,
): CropSelector {
  let size: Size = { width: 1024, height: 512 };
  let rect = leftHalf(size);
  let drag: { start: { x: number; y: number }; moving?: Rect } | undefined;

  const toOriginal = (event: PointerEvent) => {
    const bounds = stage.getBoundingClientRect();
    const scale = size.width / bounds.width;
    return {
      x: clamp(Math.round((event.clientX - bounds.left) * scale), 0, size.width),
      y: clamp(Math.round((event.clientY - bounds.top) * scale), 0, size.height),
    };
  };
  const render = () => {
    box.style.left = `${(rect.x / size.width) * 100}%`;
    box.style.top = `${(rect.y / size.height) * 100}%`;
    box.style.width = `${(rect.width / size.width) * 100}%`;
    box.style.height = `${(rect.height / size.height) * 100}%`;
    onChange(rect, size);
  };
  const inside = (point: { x: number; y: number }) =>
    point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;

  stage.onpointerdown = (event) => {
    stage.setPointerCapture(event.pointerId);
    const start = toOriginal(event);
    drag = { start, moving: inside(start) ? rect : undefined };
  };
  stage.onpointermove = (event) => {
    if (!drag) return;
    const end = toOriginal(event);
    if (drag.moving) {
      const from = drag.moving;
      rect = {
        ...from,
        x: clamp(from.x + end.x - drag.start.x, 0, size.width - from.width),
        y: clamp(from.y + end.y - drag.start.y, 0, size.height - from.height),
      };
    } else {
      const x = Math.min(drag.start.x, end.x);
      const y = Math.min(drag.start.y, end.y);
      rect = {
        x: Math.min(x, size.width - MIN_SIDE),
        y: Math.min(y, size.height - MIN_SIDE),
        width: clamp(Math.abs(end.x - drag.start.x), MIN_SIDE, size.width - Math.min(x, size.width - MIN_SIDE)),
        height: clamp(Math.abs(end.y - drag.start.y), MIN_SIDE, size.height - Math.min(y, size.height - MIN_SIDE)),
      };
    }
    render();
  };
  stage.onpointerup = stage.onpointercancel = () => (drag = undefined);
  render();
  return {
    get: () => rect,
    set: (next) => {
      rect = next;
      render();
    },
    reset: (next) => {
      size = next;
      rect = leftHalf(size);
      render();
    },
    size: () => size,
  };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}
