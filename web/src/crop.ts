// The rectangle of the framed original that gets published. Dragging inside
// it moves it; dragging anywhere else draws a new one. Coordinates are in the
// 1024x512 original.

import { HEIGHT, WIDTH } from "./fingerprint";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const LEFT_HALF: Rect = { x: 0, y: 0, width: 512, height: 512 };

export const PRESETS: { name: string; rect: Rect }[] = [
  { name: "Left half", rect: LEFT_HALF },
  { name: "Center", rect: { x: 256, y: 0, width: 512, height: 512 } },
  { name: "Right half", rect: { x: 512, y: 0, width: 512, height: 512 } },
  { name: "Whole frame", rect: { x: 0, y: 0, width: WIDTH, height: HEIGHT } },
];

const MIN_SIDE = 16;

export interface CropSelector {
  get(): Rect;
  set(rect: Rect): void;
}

export function cropSelector(stage: HTMLElement, box: HTMLElement, onChange: (rect: Rect) => void): CropSelector {
  let rect = LEFT_HALF;
  let drag: { start: { x: number; y: number }; moving?: Rect } | undefined;

  const toOriginal = (event: PointerEvent) => {
    const bounds = stage.getBoundingClientRect();
    const scale = WIDTH / bounds.width;
    return {
      x: clamp(Math.round((event.clientX - bounds.left) * scale), 0, WIDTH),
      y: clamp(Math.round((event.clientY - bounds.top) * scale), 0, HEIGHT),
    };
  };
  const render = () => {
    box.style.left = `${(rect.x / WIDTH) * 100}%`;
    box.style.top = `${(rect.y / HEIGHT) * 100}%`;
    box.style.width = `${(rect.width / WIDTH) * 100}%`;
    box.style.height = `${(rect.height / HEIGHT) * 100}%`;
    onChange(rect);
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
        x: clamp(from.x + end.x - drag.start.x, 0, WIDTH - from.width),
        y: clamp(from.y + end.y - drag.start.y, 0, HEIGHT - from.height),
      };
    } else {
      const x = Math.min(drag.start.x, end.x);
      const y = Math.min(drag.start.y, end.y);
      rect = {
        x: Math.min(x, WIDTH - MIN_SIDE),
        y: Math.min(y, HEIGHT - MIN_SIDE),
        width: clamp(Math.abs(end.x - drag.start.x), MIN_SIDE, WIDTH - Math.min(x, WIDTH - MIN_SIDE)),
        height: clamp(Math.abs(end.y - drag.start.y), MIN_SIDE, HEIGHT - Math.min(y, HEIGHT - MIN_SIDE)),
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
  };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}
