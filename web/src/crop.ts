// The rectangle of the original that gets published, and the quarter turns
// applied after the crop. Dragging inside it moves it; dragging anywhere else
// draws a new one. The rectangle is in the original's pixels; the preview
// shows the whole frame turned, so what sits in the box is what gets published.

import type { Size } from "./fingerprint";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/// Quarter turns clockwise after the crop, as Turn in crates/crop-proof.
export type Turn = 0 | 1 | 2 | 3;

export const nextTurn = (turn: Turn) => ((turn + 1) % 4) as Turn;
const inverse = (turn: Turn) => ((4 - turn) % 4) as Turn;

/// The size of a `size` image after the turn, as Turn::size.
export function turnSize(size: Size, turn: Turn): Size {
  return turn % 2 ? { width: size.height, height: size.width } : size;
}

/// Where a point of a `size` frame lands once the frame is turned. Points,
/// not pixels: corners map to corners.
export function turnPoint({ x, y }: { x: number; y: number }, { width, height }: Size, turn: Turn) {
  switch (turn) {
    case 0:
      return { x, y };
    case 1:
      return { x: height - y, y: x };
    case 2:
      return { x: width - x, y: height - y };
    case 3:
      return { x: y, y: width - x };
  }
}

/// Where `rect` of a `size` frame lands once the frame is turned. Cropping
/// then turning gives the pixels that turning then cropping here gives.
export function turnRect(rect: Rect, size: Size, turn: Turn): Rect {
  const a = turnPoint(rect, size, turn);
  const b = turnPoint({ x: rect.x + rect.width, y: rect.y + rect.height }, size, turn);
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(a.x - b.x), height: Math.abs(a.y - b.y) };
}

/// The rectangle of the original under `rect` of the turned frame.
export function unturnRect(rect: Rect, size: Size, turn: Turn): Rect {
  return turnRect(rect, turnSize(size, turn), inverse(turn));
}

/// The left half, rounded down, as Rect::left_half in crates/crop-proof.
export function leftHalf({ width, height }: Size): Rect {
  return { x: 0, y: 0, width: Math.max(1, Math.floor(width / 2)), height };
}

/// Presets name what the preview shows, so each takes the turned frame's size.
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
  turn(): Turn;
  /// A quarter turn clockwise more, from 270 degrees back to none.
  rotate(): void;
  /// A new original: its size, the crop back to its left half, and no turn.
  reset(size: Size): void;
  size(): Size;
}

export function cropSelector(
  stage: HTMLElement,
  canvas: HTMLCanvasElement,
  box: HTMLElement,
  onChange: (rect: Rect, size: Size, turn: Turn) => void,
): CropSelector {
  let size: Size = { width: 1024, height: 512 };
  let rect = leftHalf(size);
  let turn: Turn = 0;
  let drag: { start: { x: number; y: number }; moving?: Rect } | undefined;

  const toOriginal = (event: PointerEvent) => {
    const bounds = stage.getBoundingClientRect();
    const shown = turnSize(size, turn);
    const scale = shown.width / bounds.width;
    const point = {
      x: clamp(Math.round((event.clientX - bounds.left) * scale), 0, shown.width),
      y: clamp(Math.round((event.clientY - bounds.top) * scale), 0, shown.height),
    };
    return turnPoint(point, shown, inverse(turn));
  };
  const render = () => {
    const shown = turnSize(size, turn);
    // The canvas holds the original's pixels, so the preview turns it in CSS.
    stage.style.aspectRatio = `${shown.width} / ${shown.height}`;
    // A tall frame narrows instead, so the controls under it stay on screen.
    stage.style.width = `min(100%, ${(60 * shown.width) / shown.height}svh)`;
    canvas.style.width = `${(size.width / shown.width) * 100}%`;
    canvas.style.transform = `translate(-50%, -50%) rotate(${turn * 90}deg)`;
    const at = turnRect(rect, size, turn);
    box.style.left = `${(at.x / shown.width) * 100}%`;
    box.style.top = `${(at.y / shown.height) * 100}%`;
    box.style.width = `${(at.width / shown.width) * 100}%`;
    box.style.height = `${(at.height / shown.height) * 100}%`;
    onChange(rect, size, turn);
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
    turn: () => turn,
    rotate: () => {
      turn = nextTurn(turn);
      render();
    },
    reset: (next) => {
      size = next;
      rect = leftHalf(size);
      turn = 0;
      render();
    },
    size: () => size,
  };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}
