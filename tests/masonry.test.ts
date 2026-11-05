import assert from "node:assert/strict";
import { test } from "node:test";
import { layoutMasonry, visibleIndices } from "../src/masonry.ts";
import { base64UrlEncode, clampColumns, sharpEdge } from "../src/media.ts";

test("columns share one width and stack into the shortest column", () => {
  const layout = layoutMasonry(
    [
      { width: 100, height: 100 },
      { width: 100, height: 200 },
      { width: 200, height: 100 },
    ],
    2,
    208,
    8,
  );
  assert.equal(layout.colWidth, 100);
  assert.equal(layout.positions[0].w, layout.positions[1].w);
  assert.equal(layout.positions[0].x, 0);
  assert.equal(layout.positions[1].x, 108);
  assert.equal(layout.positions[0].h, 100);
  assert.equal(layout.positions[1].h, 200);
  assert.equal(layout.positions[2].x, 0);
  assert.equal(layout.positions[2].y, 108);
  assert.equal(layout.positions[2].h, 50);
  assert.equal(layout.totalHeight, 200);
});

test("empty and single-column layouts stay finite", () => {
  const empty = layoutMasonry([], 4, 800, 8);
  assert.equal(empty.positions.length, 0);
  assert.equal(empty.totalHeight, 0);
  const one = layoutMasonry([{ width: 0, height: 0 }], 1, 400, 8);
  assert.equal(one.positions.length, 1);
  assert.ok(one.positions[0].h > 0);
});

test("10k layout stays small in the DOM window", () => {
  const items = Array.from({ length: 10_000 }, (_, i) => ({
    width: 300,
    height: 180 + (i % 7) * 36,
  }));
  const start = performance.now();
  const layout = layoutMasonry(items, 5, 1600, 8);
  const elapsed = performance.now() - start;
  assert.equal(layout.positions.length, 10_000);
  assert.ok(elapsed < 250, `layout took ${elapsed}ms`);
  const visible = visibleIndices(layout.positions, 80_000, 900, 700);
  assert.ok(visible.length > 0);
  assert.ok(visible.length < 400, `visible ${visible.length}`);
  const far = visibleIndices(layout.positions, 0, 0, 0);
  assert.ok(far.length < 20);
});

test("column clamp and sharp edge stay inside the cache bounds", () => {
  assert.equal(clampColumns(0), 1);
  assert.equal(clampColumns(99), 12);
  assert.equal(clampColumns(4.4), 4);
  assert.equal(sharpEdge(200, 2), 400);
  assert.equal(sharpEdge(20, 1), 64);
  assert.equal(sharpEdge(2000, 3), 960);
});

test("base64url matches the Rust URL_SAFE_NO_PAD decoder", () => {
  const encode = (text: string) => base64UrlEncode(new TextEncoder().encode(text));
  assert.equal(encode("hello"), "aGVsbG8");
  assert.equal(encode("a/b c"), "YS9iIGM");
  assert.equal(encode("Photo Wall"), "UGhvdG8gV2FsbA");
  assert.equal(encode("C:\\Photos\\图片.png"), "QzpcUGhvdG9zXOWbvueJhy5wbmc");
  assert.equal(base64UrlEncode(new Uint8Array([251, 239, 255])), "--__");
});
