import { expect, test } from "vitest";

import { leftHalf, nextTurn, turnPoint, turnRect, turnSize, unturnRect, type Rect, type Turn } from "../src/crop";

const TURNS: Turn[] = [0, 1, 2, 3];

/// Turn::source in crates/crop-proof: the pixel of a w x h image that lands on (u, v).
function source(turn: Turn, w: number, h: number, u: number, v: number): [number, number] {
  return [
    [u, v],
    [v, h - 1 - u],
    [w - 1 - u, h - 1 - v],
    [w - 1 - v, u],
  ][turn] as [number, number];
}

/// A w x h image whose pixel (x, y) holds y * w + x, cropped then turned as crop_proof::edit does.
function edit(w: number, rect: Rect, turn: Turn): number[][] {
  const out = turnSize(rect, turn);
  return Array.from({ length: out.height }, (_, v) =>
    Array.from({ length: out.width }, (_, u) => {
      const [x, y] = source(turn, rect.width, rect.height, u, v);
      return (rect.y + y) * w + rect.x + x;
    }),
  );
}

test("a turn cycles clockwise and back to none", () => {
  expect([0, 1, 2, 3].map((turn) => nextTurn(turn as Turn))).toEqual([1, 2, 3, 0]);
});

test("odd turns swap the sides", () => {
  expect(TURNS.map((turn) => turnSize({ width: 300, height: 200 }, turn))).toEqual([
    { width: 300, height: 200 },
    { width: 200, height: 300 },
    { width: 300, height: 200 },
    { width: 200, height: 300 },
  ]);
});

test("each pixel lands where Turn::source takes it from", () => {
  const size = { width: 5, height: 3 };
  for (const turn of TURNS) {
    const turned = turnSize(size, turn);
    for (let v = 0; v < turned.height; v++) {
      for (let u = 0; u < turned.width; u++) {
        const [x, y] = source(turn, size.width, size.height, u, v);
        expect(turnRect({ x, y, width: 1, height: 1 }, size, turn)).toEqual({ x: u, y: v, width: 1, height: 1 });
      }
    }
  }
});

test("the box on the turned preview holds the published pixels", () => {
  const size = { width: 7, height: 5 };
  const rect = { x: 1, y: 2, width: 4, height: 2 };
  const frame = { x: 0, y: 0, ...size };
  for (const turn of TURNS) {
    const preview = edit(size.width, frame, turn);
    const at = turnRect(rect, size, turn);
    const boxed = preview.slice(at.y, at.y + at.height).map((row) => row.slice(at.x, at.x + at.width));
    expect(boxed).toEqual(edit(size.width, rect, turn));
  }
});

test("unturning undoes turning, for points and rectangles", () => {
  const size = { width: 640, height: 480 };
  const rect = { x: 10, y: 20, width: 300, height: 200 };
  for (const turn of TURNS) {
    expect(unturnRect(turnRect(rect, size, turn), size, turn)).toEqual(rect);
    const turned = turnPoint({ x: 10, y: 20 }, size, turn);
    expect(turnPoint(turned, turnSize(size, turn), ((4 - turn) % 4) as Turn)).toEqual({ x: 10, y: 20 });
  }
});

test("the left half of the preview turned clockwise is the original's bottom half", () => {
  const size = { width: 641, height: 480 };
  expect(unturnRect(leftHalf(turnSize(size, 1)), size, 1)).toEqual({ x: 0, y: 240, width: 641, height: 240 });
  expect(unturnRect(leftHalf(turnSize(size, 3)), size, 3)).toEqual({ x: 0, y: 0, width: 641, height: 240 });
});
