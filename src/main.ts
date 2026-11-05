import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { layoutMasonry, visibleIndices, type Positioned } from "./masonry";
import {
  GAP,
  GUTTER,
  LOW_EDGE,
  clampColumns,
  detectPlatform,
  mediaUrl,
  sharpEdge,
} from "./media";

type Settings = {
  lastFolder: string | null;
  includeSubfolders: boolean;
  columnCount: number;
};

type ImageRecord = {
  path: string;
  name: string;
  width: number;
  height: number;
  bytes: number;
  modifiedMs: number;
};

type ScanProgress = {
  phase: string;
  done: number;
  total: number;
  generation: number;
};

const app = document.querySelector("#app") as HTMLElement;
const folderEl = document.querySelector("#folder") as HTMLElement;
const subfolderInput = document.querySelector("#subfolders") as HTMLInputElement;
const colCountEl = document.querySelector("#col-count") as HTMLElement;
const colDec = document.querySelector("#col-dec") as HTMLButtonElement;
const colInc = document.querySelector("#col-inc") as HTMLButtonElement;
const openBtn = document.querySelector("#open") as HTMLButtonElement;
const emptyOpen = document.querySelector("#empty-open") as HTMLButtonElement;
const progressEl = document.querySelector("#progress") as HTMLElement;
const progressBar = document.querySelector("#progress-bar") as HTMLElement;
const progressLabel = document.querySelector("#progress-label") as HTMLElement;
const banner = document.querySelector("#banner") as HTMLElement;
const scroller = document.querySelector("#scroller") as HTMLElement;
const emptyEl = document.querySelector("#empty") as HTMLElement;
const emptyTitle = document.querySelector("#empty-title") as HTMLElement;
const emptyBody = document.querySelector("#empty-body") as HTMLElement;
const spacer = document.querySelector("#spacer") as HTMLElement;
const countEl = document.querySelector("#count") as HTMLElement;
const dropveil = document.querySelector("#dropveil") as HTMLElement;
const lightbox = document.querySelector("#lightbox") as HTMLElement;
const lbImg = document.querySelector("#lb-img") as HTMLImageElement;
const lbName = document.querySelector("#lb-name") as HTMLElement;
const lbIndex = document.querySelector("#lb-index") as HTMLElement;
const lbPrev = document.querySelector("#lb-prev") as HTMLButtonElement;
const lbNext = document.querySelector("#lb-next") as HTMLButtonElement;
const lbClose = document.querySelector("#lb-close") as HTMLButtonElement;
const lbStage = document.querySelector("#lb-stage") as HTMLElement;

const platform = detectPlatform();
let folder: string | null = null;
let includeSubfolders = false;
let columnCount = 5;
let images: ImageRecord[] = [];
let positions: Positioned[] = [];
let scanToken = 0;
let scanActive = false;
let progressGen = -1;
let lightboxOpen = false;
let lightboxIndex = 0;
let savedScroll = 0;
let widthAtOpen = 0;
let lastWidth = -1;
let layingOut = false;
let saveTimer = 0;
const mounted = new Map<number, HTMLButtonElement>();
let preloaders: HTMLImageElement[] = [];
let scrollRaf = 0;
let wheelAcc = 0;

function formatError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  try {
    return JSON.stringify(error);
  } catch {
    return "Something went wrong";
  }
}

function showBanner(message: string): void {
  banner.hidden = false;
  banner.textContent = message;
}

function hideBanner(): void {
  banner.hidden = true;
  banner.textContent = "";
}

function setEmpty(title: string, body: string): void {
  emptyTitle.textContent = title;
  emptyBody.textContent = body;
}

function setCount(): void {
  const n = images.length;
  countEl.textContent = `${n.toLocaleString()} photo${n === 1 ? "" : "s"}`;
}

function showProgress(label: string, ratio: number | null): void {
  progressEl.hidden = false;
  progressLabel.textContent = label;
  if (ratio === null) {
    progressBar.classList.add("indeterminate");
    progressBar.style.width = "";
  } else {
    progressBar.classList.remove("indeterminate");
    progressBar.style.width = `${Math.max(0, Math.min(100, ratio * 100))}%`;
  }
  app.setAttribute("aria-busy", "true");
}

function hideProgress(): void {
  progressEl.hidden = true;
  progressBar.classList.remove("indeterminate");
  app.removeAttribute("aria-busy");
}

function contentWidth(): number {
  return Math.max(1, scroller.clientWidth - GUTTER * 2);
}

function syncEmpty(): void {
  const show = images.length === 0;
  emptyEl.hidden = !show;
  spacer.hidden = show;
}

function clearCards(): void {
  for (const el of mounted.values()) el.remove();
  mounted.clear();
}

function thumbUrl(item: ImageRecord, edge: number): string {
  return mediaUrl("thumb", edge, item.path, item.modifiedMs, item.bytes, platform);
}

function fullUrl(item: ImageRecord): string {
  return mediaUrl("full", 0, item.path, item.modifiedMs, item.bytes, platform);
}

function createCard(index: number): HTMLButtonElement {
  const item = images[index];
  const pos = positions[index];
  const el = document.createElement("button");
  el.type = "button";
  el.className = "card";
  el.setAttribute("aria-label", item.name);
  el.style.transform = `translate3d(${pos.x}px, ${pos.y}px, 0)`;
  el.style.width = `${pos.w}px`;
  el.style.height = `${pos.h}px`;
  el.addEventListener("mousedown", (event) => {
    if (event.button === 0) event.preventDefault();
  });
  el.addEventListener("click", () => openLightbox(index));

  const low = document.createElement("img");
  low.className = "thumb low";
  low.alt = "";
  low.decoding = "async";
  low.draggable = false;
  low.setAttribute("fetchpriority", "high");

  const sharp = document.createElement("img");
  sharp.className = "thumb sharp";
  sharp.alt = item.name;
  sharp.decoding = "async";
  sharp.draggable = false;
  sharp.setAttribute("fetchpriority", "low");

  let failures = 0;
  const markFailed = () => {
    failures += 1;
    if (failures >= 2) el.classList.add("failed");
  };
  low.addEventListener("error", () => {
    markFailed();
    const edge = sharpEdge(pos.w, window.devicePixelRatio);
    if (edge > LOW_EDGE) sharp.src = thumbUrl(item, edge);
  });
  low.addEventListener("load", () => {
    const edge = sharpEdge(pos.w, window.devicePixelRatio);
    if (edge <= LOW_EDGE + 8) {
      low.classList.add("ready");
      return;
    }
    sharp.src = thumbUrl(item, edge);
  });
  sharp.addEventListener("load", () => sharp.classList.add("ready"));
  sharp.addEventListener("error", markFailed);

  low.src = thumbUrl(item, LOW_EDGE);
  el.append(low, sharp);
  return el;
}

function renderVisible(): void {
  if (!images.length || !positions.length) {
    clearCards();
    return;
  }
  const buffer = Math.max(400, scroller.clientHeight * 0.75);
  const wanted = new Set(
    visibleIndices(positions, scroller.scrollTop, scroller.clientHeight, buffer),
  );
  for (const [index, el] of mounted) {
    if (!wanted.has(index)) {
      el.remove();
      mounted.delete(index);
    }
  }
  for (const index of wanted) {
    if (mounted.has(index)) continue;
    const el = createCard(index);
    spacer.appendChild(el);
    mounted.set(index, el);
  }
}

function captureAnchor(): { index: number; offset: number } | null {
  if (!positions.length) return null;
  const top = scroller.scrollTop;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (pos.y + pos.h >= top) return { index: i, offset: top - pos.y };
  }
  return { index: positions.length - 1, offset: 0 };
}

function relayout(preserve: boolean): void {
  if (layingOut || lightboxOpen) return;
  layingOut = true;
  try {
    const anchor = preserve ? captureAnchor() : null;
    const apply = (width: number) => {
      const layout = layoutMasonry(
        images.map((item) => ({ width: item.width, height: item.height })),
        columnCount,
        width,
        GAP,
      );
      positions = layout.positions;
      spacer.style.height = `${layout.totalHeight}px`;
      spacer.style.width = `${width}px`;
    };
    clearCards();
    apply(contentWidth());
    const settled = contentWidth();
    if (Math.abs(settled - (parseFloat(spacer.style.width) || 0)) > 1) apply(settled);
    if (anchor) {
      const pos = positions[anchor.index];
      if (pos) scroller.scrollTop = Math.max(0, pos.y + anchor.offset);
    }
    lastWidth = contentWidth();
    syncEmpty();
    renderVisible();
  } finally {
    layingOut = false;
  }
}

function scheduleSave(): void {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    void saveSettings();
  }, 200);
}

async function saveSettings(): Promise<void> {
  const settings: Settings = {
    lastFolder: folder,
    includeSubfolders,
    columnCount,
  };
  await invoke("save_settings", { settings });
}

function changeColumns(delta: number): void {
  if (lightboxOpen) return;
  const next = clampColumns(columnCount + delta);
  if (next === columnCount) return;
  columnCount = next;
  colCountEl.textContent = String(columnCount);
  scheduleSave();
  relayout(true);
}

async function openFolder(path: string): Promise<void> {
  const token = ++scanToken;
  scanActive = true;
  hideLightbox();
  hideBanner();
  folder = path;
  folderEl.textContent = path;
  folderEl.title = path;
  images = [];
  positions = [];
  clearCards();
  scroller.scrollTop = 0;
  setCount();
  setEmpty("Reading photos…", path);
  syncEmpty();
  showProgress("Listing photos…", null);
  try {
    const records = await invoke<ImageRecord[]>("scan_folder", {
      path,
      recursive: includeSubfolders,
    });
    if (token !== scanToken) return;
    images = records;
    setCount();
    if (!records.length) {
      setEmpty(
        "No supported photos",
        includeSubfolders
          ? "This folder has no JPG, PNG, WEBP, or GIF files."
          : "Try turning on subfolders, or choose a folder that contains photos.",
      );
    } else {
      setEmpty("Drop a photo folder", "JPG, JPEG, PNG, WEBP, and GIF. Hidden files are skipped.");
    }
    const leaf = path.split(/[/\\]/).filter(Boolean).pop() ?? path;
    document.title = `PhotoWall — ${leaf}`;
    try {
      await getCurrentWindow().setTitle(document.title);
    } catch {
      /* The native title is optional if the window permission is unavailable. */
    }
    relayout(false);
    await saveSettings();
  } catch (error) {
    if (token !== scanToken) return;
    const message = formatError(error);
    if (message.toLowerCase().includes("cancel")) return;
    setEmpty("Could not open that folder", message);
    syncEmpty();
    showBanner(message);
  } finally {
    if (token === scanToken) {
      scanActive = false;
      hideProgress();
    }
  }
}

async function chooseFolder(): Promise<void> {
  try {
    const picked = await invoke<string | null>("pick_folder", { start: folder });
    if (picked) await openFolder(picked);
  } catch (error) {
    showBanner(formatError(error));
  }
}

function hideLightbox(): void {
  lightboxOpen = false;
  lightbox.hidden = true;
  lbImg.removeAttribute("src");
  preloaders = [];
}

function closeLightbox(): void {
  if (!lightboxOpen) return;
  const scroll = savedScroll;
  const widthChanged = Math.abs(contentWidth() - widthAtOpen) > 1;
  hideLightbox();
  if (widthChanged) relayout(false);
  scroller.scrollTop = scroll;
  requestAnimationFrame(() => {
    scroller.scrollTop = scroll;
  });
}

function showLightboxImage(index: number): void {
  const item = images[index];
  if (!item) return;
  lightboxIndex = index;
  lbName.textContent = item.name;
  lbIndex.textContent = `${index + 1} / ${images.length.toLocaleString()}`;
  lbImg.classList.add("waiting");
  lbImg.alt = item.name;
  lbImg.src = fullUrl(item);
  preloaders = [];
  for (const step of [-1, 1]) {
    const neighbor = images[index + step];
    if (!neighbor) continue;
    const img = new Image();
    img.src = fullUrl(neighbor);
    preloaders.push(img);
  }
}

function openLightbox(index: number): void {
  if (!images[index]) return;
  savedScroll = scroller.scrollTop;
  widthAtOpen = contentWidth();
  lightboxOpen = true;
  lightbox.hidden = false;
  showLightboxImage(index);
}

function stepLightbox(delta: number): void {
  if (!lightboxOpen || images.length === 0) return;
  const next = (lightboxIndex + delta + images.length) % images.length;
  showLightboxImage(next);
}

lbImg.addEventListener("load", () => lbImg.classList.remove("waiting"));
lbImg.addEventListener("error", () => {
  lbImg.classList.remove("waiting");
  lbName.textContent = `${lbName.textContent} — could not display`;
});

openBtn.addEventListener("click", () => void chooseFolder());
emptyOpen.addEventListener("click", () => void chooseFolder());
colDec.addEventListener("click", () => changeColumns(-1));
colInc.addEventListener("click", () => changeColumns(1));
lbPrev.addEventListener("click", () => stepLightbox(-1));
lbNext.addEventListener("click", () => stepLightbox(1));
lbClose.addEventListener("click", () => closeLightbox());
lightbox.addEventListener("click", (event) => {
  if (event.target === lightbox || event.target === lbStage) closeLightbox();
});

subfolderInput.addEventListener("change", () => {
  includeSubfolders = subfolderInput.checked;
  scheduleSave();
  if (!folder) return;
  if (lightboxOpen) closeLightbox();
  void openFolder(folder);
});

scroller.addEventListener(
  "scroll",
  () => {
    if (scrollRaf) return;
    scrollRaf = requestAnimationFrame(() => {
      scrollRaf = 0;
      if (!lightboxOpen) renderVisible();
    });
  },
  { passive: true },
);

window.addEventListener(
  "wheel",
  (event) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    if (lightboxOpen) return;
    wheelAcc += event.deltaY;
    const step = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? 100 : 1;
    while (wheelAcc >= step) {
      wheelAcc -= step;
      changeColumns(-1);
    }
    while (wheelAcc <= -step) {
      wheelAcc += step;
      changeColumns(1);
    }
  },
  { passive: false },
);

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && lightboxOpen) {
    event.preventDefault();
    closeLightbox();
    return;
  }
  if (lightboxOpen && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
    event.preventDefault();
    stepLightbox(event.key === "ArrowLeft" ? -1 : 1);
    return;
  }
  if (lightboxOpen) return;
  const plus = event.key === "+" || event.key === "=" || event.code === "NumpadAdd";
  const minus = event.key === "-" || event.key === "_" || event.code === "NumpadSubtract";
  if (!plus && !minus) return;
  event.preventDefault();
  changeColumns(plus ? 1 : -1);
});

new ResizeObserver(() => {
  if (lightboxOpen || layingOut || images.length === 0) return;
  const width = contentWidth();
  if (Math.abs(width - lastWidth) < 1) return;
  relayout(true);
}).observe(scroller);

function onProgress(payload: ScanProgress): void {
  if (!scanActive || payload.generation < progressGen) return;
  progressGen = payload.generation;
  if (payload.phase === "listing") {
    const found = payload.done.toLocaleString();
    showProgress(payload.total ? `Found ${found} photos` : `Listing photos… ${found}`, null);
    return;
  }
  const ratio = payload.total > 0 ? payload.done / payload.total : null;
  showProgress(
    `Reading sizes ${payload.done.toLocaleString()} / ${payload.total.toLocaleString()}`,
    ratio,
  );
}

async function boot(): Promise<void> {
  await listen<ScanProgress>("scan-progress", (event) => onProgress(event.payload));
  await getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "enter" || payload.type === "over") {
      dropveil.hidden = false;
      return;
    }
    dropveil.hidden = true;
    if (payload.type !== "drop") return;
    const dropped = payload.paths[0];
    if (!dropped) return;
    void (async () => {
      try {
        const folderPath = await invoke<string>("normalize_drop_path", { path: dropped });
        await openFolder(folderPath);
      } catch (error) {
        showBanner(formatError(error));
      }
    })();
  });

  try {
    const settings = await invoke<Settings>("load_settings");
    columnCount = clampColumns(settings.columnCount);
    includeSubfolders = Boolean(settings.includeSubfolders);
    subfolderInput.checked = includeSubfolders;
    colCountEl.textContent = String(columnCount);
    if (settings.lastFolder) await openFolder(settings.lastFolder);
  } catch (error) {
    showBanner(formatError(error));
  }
}

void boot();
