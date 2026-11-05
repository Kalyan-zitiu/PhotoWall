export type Box = {
  width: number;
  height: number;
};

export type Positioned = {
  index: number;
  x: number;
  y: number;
  w: number;
  h: number;
};

export type MasonryLayout = {
  positions: Positioned[];
  totalHeight: number;
  colWidth: number;
};

const FALLBACK_W = 4;
const FALLBACK_H = 3;

/** Equal-width waterfall: each photo goes into the current shortest column. */
export function layoutMasonry(
  items: readonly Box[],
  columnCount: number,
  containerWidth: number,
  gap: number,
): MasonryLayout {
  const cols = Math.max(1, Math.floor(columnCount));
  const width = Math.max(1, containerWidth);
  const colWidth = (width - gap * (cols - 1)) / cols;
  const heights = new Array<number>(cols).fill(0);
  const positions = new Array<Positioned>(items.length);

  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    const aw = item.width > 0 ? item.width : FALLBACK_W;
    const ah = item.height > 0 ? item.height : FALLBACK_H;
    const h = colWidth * (ah / aw);
    let col = 0;
    for (let c = 1; c < cols; c++) {
      if (heights[c] < heights[col]) col = c;
    }
    const x = col * (colWidth + gap);
    const y = heights[col];
    positions[i] = { index: i, x, y, w: colWidth, h };
    heights[col] = y + h + gap;
  }

  const tallest = heights.length ? Math.max(...heights) : 0;
  const totalHeight = items.length ? Math.max(0, tallest - gap) : 0;
  return { positions, totalHeight, colWidth };
}

/** Indices whose boxes intersect the viewport, plus a buffer above and below. */
export function visibleIndices(
  positions: readonly { y: number; h: number }[],
  scrollTop: number,
  viewport: number,
  buffer: number,
): number[] {
  const y0 = scrollTop - buffer;
  const y1 = scrollTop + viewport + buffer;
  const out: number[] = [];
  for (let i = 0; i < positions.length; i++) {
    const p = positions[i];
    if (p.y + p.h >= y0 && p.y <= y1) out.push(i);
  }
  return out;
}
