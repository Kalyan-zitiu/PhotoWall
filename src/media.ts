export const LOW_EDGE = 64;
export const MAX_SHARP_EDGE = 960;
export const MIN_COLUMNS = 1;
export const MAX_COLUMNS = 12;
export const GAP = 8;
export const GUTTER = 12;

export function clampColumns(value: number): number {
  if (!Number.isFinite(value)) return 5;
  return Math.min(MAX_COLUMNS, Math.max(MIN_COLUMNS, Math.round(value)));
}

export function sharpEdge(colWidth: number, devicePixelRatio: number): number {
  const dpr = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const edge = Math.round(colWidth * dpr);
  return Math.max(LOW_EDGE, Math.min(MAX_SHARP_EDGE, edge));
}

/** URL-safe base64 without padding. Matches the Rust `URL_SAFE_NO_PAD` decoder. */
export function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

export function mediaUrl(
  kind: "thumb" | "full",
  edge: number,
  filePath: string,
  modifiedMs: number,
  bytes: number,
  platform: "windows" | "unix",
): string {
  const b64 = base64UrlEncode(new TextEncoder().encode(filePath));
  const mtime = Math.max(0, Math.floor(modifiedMs));
  const len = Math.max(0, Math.floor(bytes));
  const path = `${kind}/${Math.max(0, Math.floor(edge))}/${mtime}/${len}/${b64}`;
  if (platform === "windows") return `http://thumb.localhost/${path}`;
  return `thumb://localhost/${path}`;
}

export function detectPlatform(): "windows" | "unix" {
  return navigator.userAgent.includes("Windows") ? "windows" : "unix";
}
