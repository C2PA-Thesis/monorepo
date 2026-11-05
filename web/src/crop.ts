// The rectangle of the framed original that gets published, drawn by
// dragging on the preview. Coordinates are in the 1024x512 original.

import { HEIGHT, WIDTH } from "./fingerprint";

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const LEFT_HALF: Rect = { x: 0, y: 0, width: 512, height: 512 };
const MIN_SIDE = 16;

export interface CropSelector {
  get(): Rect;
}

export function cropSelector(stage: HTMLElement, box: HTMLElement, onChange: (rect: Rect) => void): CropSelector {
  let rect = LEFT_HALF;
  let start: { x: number; y: number } | undefined;

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

  stage.onpointerdown = (event) => {
    stage.setPointerCapture(event.pointerId);
    start = toOriginal(event);
  };
  stage.onpointermove = (event) => {
    if (!start) return;
    const end = toOriginal(event);
    const x = Math.min(start.x, end.x);
    const y = Math.min(start.y, end.y);
    rect = {
      x,
      y,
      width: clamp(Math.abs(end.x - start.x), MIN_SIDE, WIDTH - x),
      height: clamp(Math.abs(end.y - start.y), MIN_SIDE, HEIGHT - y),
    };
    render();
  };
  stage.onpointerup = stage.onpointercancel = () => (start = undefined);
  render();
  return { get: () => rect };
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(Math.max(value, low), high);
}
