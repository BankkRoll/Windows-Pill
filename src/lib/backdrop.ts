/**
 * Turns the core's wallpaper sample into the glass material's image.
 *
 * The sample covers a whole monitor, so it's blurred once here and each
 * window shows its slice of it by offset. Moving a window only changes that
 * offset; nothing is re-sampled or re-blurred, so dragging stays smooth.
 *
 * The sample is either the live desktop or a picked picture (the core
 * decodes both into the same shape). Instead, or when the wallpaper can't
 * carry glass (black, or a flat single color), Pill paints one of its own
 * gradients: the same on every machine.
 */

import { useMemo } from "react";
import { usePillStore } from "../store";
import type { Backdrop, GlassSource } from "./native";

/** A blurred monitor backdrop ready to paint. */
export interface Wallpaper {
  /** Data URL of the blurred image. */
  url: string;
  /** Monitor `[x, y, width, height]` in physical pixels. */
  monitor: [number, number, number, number];
  scale: number;
  /** What was painted: the desktop, a picked picture or a Pill gradient. */
  source: "wallpaper" | "image" | "pill";
}

/** One of Pill's gradient backdrops. */
export interface GlassGradient {
  /** Stable id stored in the config; mirrors Rust `config::GLASS_GRADIENTS`. */
  id: string;
  label: string;
  /** Diagonal base fill, top-left to bottom-right. */
  base: readonly [string, string, string];
  /** Soft lights screened over the base: `[x, y, radius, color]`, as fractions of the canvas. */
  glows: readonly (readonly [number, number, number, string])[];
}

/** Pill's gradient backdrops, in picker order. The first is the default. */
export const GLASS_GRADIENTS: readonly GlassGradient[] = [
  {
    id: "blue",
    label: "Blue",
    base: ["#1b2a44", "#14213a", "#0d1830"],
    glows: [
      [0.72, 0.62, 0.55, "rgba(37, 99, 235, 0.85)"],
      [0.92, 0.95, 0.35, "rgba(30, 64, 175, 0.9)"],
      [0.38, 0.22, 0.45, "rgba(99, 102, 241, 0.35)"],
      [0.12, 0.85, 0.4, "rgba(20, 120, 140, 0.35)"],
      [0.55, 0.05, 0.3, "rgba(148, 163, 184, 0.22)"],
    ],
  },
  {
    id: "violet",
    label: "Violet",
    base: ["#241a3e", "#1b1434", "#120d27"],
    glows: [
      [0.68, 0.6, 0.55, "rgba(139, 92, 246, 0.75)"],
      [0.9, 0.92, 0.35, "rgba(91, 33, 182, 0.85)"],
      [0.18, 0.25, 0.42, "rgba(217, 70, 239, 0.32)"],
      [0.35, 0.9, 0.4, "rgba(56, 189, 248, 0.22)"],
      [0.6, 0.06, 0.3, "rgba(196, 181, 253, 0.2)"],
    ],
  },
  {
    id: "ember",
    label: "Ember",
    base: ["#2c1711", "#22110d", "#170b08"],
    glows: [
      [0.66, 0.68, 0.55, "rgba(234, 88, 12, 0.72)"],
      [0.94, 0.9, 0.35, "rgba(185, 28, 28, 0.8)"],
      [0.22, 0.3, 0.42, "rgba(244, 63, 94, 0.35)"],
      [0.45, 0.05, 0.32, "rgba(251, 191, 36, 0.24)"],
      [0.1, 0.9, 0.35, "rgba(120, 53, 15, 0.5)"],
    ],
  },
  {
    id: "forest",
    label: "Forest",
    base: ["#12271f", "#0e201a", "#091612"],
    glows: [
      [0.7, 0.64, 0.55, "rgba(16, 185, 129, 0.55)"],
      [0.95, 0.95, 0.35, "rgba(6, 95, 70, 0.9)"],
      [0.16, 0.22, 0.45, "rgba(132, 204, 22, 0.26)"],
      [0.34, 0.9, 0.4, "rgba(20, 184, 166, 0.3)"],
      [0.58, 0.04, 0.3, "rgba(187, 247, 208, 0.16)"],
    ],
  },
  {
    id: "graphite",
    label: "Graphite",
    base: ["#2b2e34", "#222429", "#18191d"],
    glows: [
      [0.7, 0.6, 0.55, "rgba(148, 163, 184, 0.34)"],
      [0.92, 0.95, 0.35, "rgba(71, 85, 105, 0.6)"],
      [0.22, 0.2, 0.42, "rgba(226, 232, 240, 0.16)"],
      [0.15, 0.88, 0.38, "rgba(100, 116, 139, 0.3)"],
    ],
  },
  {
    id: "dusk",
    label: "Dusk",
    base: ["#2b1b34", "#20182f", "#121a2e"],
    glows: [
      [0.74, 0.7, 0.5, "rgba(251, 113, 133, 0.55)"],
      [0.92, 0.95, 0.32, "rgba(124, 58, 237, 0.6)"],
      [0.2, 0.86, 0.45, "rgba(59, 130, 246, 0.45)"],
      [0.48, 0.08, 0.4, "rgba(250, 204, 21, 0.17)"],
    ],
  },
];

/**
 * Looks up a gradient, falling back to the default for unknown ids.
 *
 * @param id - Gradient id from the config.
 * @returns The gradient.
 */
export function glassGradient(id: string): GlassGradient {
  return GLASS_GRADIENTS.find((g) => g.id === id) ?? GLASS_GRADIENTS[0]!;
}

/** Blur radius in CSS pixels, matching how soft Mica looks. */
const BLUR_CSS_PX = 22;
/** Rendered at 1/4 of the monitor's CSS size; the blur hides the difference. */
const DOWNSCALE = 4;

/**
 * Whether a wallpaper sample has enough light and variation for glass to
 * read as glass. Very dark or near-uniform wallpapers make it look like a
 * flat gray panel.
 *
 * @param b - Monitor sample.
 * @returns `true` when the wallpaper is a good glass source.
 */
export function wallpaperCarriesGlass(b: Backdrop): boolean {
  const n = b.width * b.height;
  if (n === 0) return false;
  let sum = 0;
  let sumSq = 0;
  for (let i = 0; i < n; i++) {
    const r = b.pixels[i * 3] ?? 0;
    const g = b.pixels[i * 3 + 1] ?? 0;
    const bl = b.pixels[i * 3 + 2] ?? 0;
    const y = (0.2126 * r + 0.7152 * g + 0.0722 * bl) / 255;
    sum += y;
    sumSq += y * y;
  }
  const mean = sum / n;
  const spread = Math.sqrt(Math.max(0, sumSq / n - mean * mean));
  return mean > 0.1 && spread > 0.035;
}

function canvas(w: number, h: number): [HTMLCanvasElement, CanvasRenderingContext2D] | null {
  const c = document.createElement("canvas");
  c.width = Math.max(1, Math.round(w));
  c.height = Math.max(1, Math.round(h));
  const ctx = c.getContext("2d");
  return ctx ? [c, ctx] : null;
}

/** Paints the monitor sample, upscaled and blurred, into `ctx`. */
function paintSample(ctx: CanvasRenderingContext2D, w: number, h: number, b: Backdrop): boolean {
  const tiny = canvas(b.width, b.height);
  if (!tiny) return false;
  const [tc, tctx] = tiny;
  const img = tctx.createImageData(b.width, b.height);
  for (let i = 0, j = 0; i < b.width * b.height; i++, j += 3) {
    img.data[i * 4] = b.pixels[j] ?? 0;
    img.data[i * 4 + 1] = b.pixels[j + 1] ?? 0;
    img.data[i * 4 + 2] = b.pixels[j + 2] ?? 0;
    img.data[i * 4 + 3] = 255;
  }
  tctx.putImageData(img, 0, 0);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  // An unblurred base first: blur fades toward transparent at the edges,
  // and the base fills that in so borders stay as bright as the middle.
  ctx.drawImage(tc, 0, 0, w, h);
  ctx.filter = `blur(${BLUR_CSS_PX / DOWNSCALE}px)`;
  ctx.drawImage(tc, 0, 0, w, h);
  ctx.filter = "none";
  return true;
}

/**
 * Paints a Pill gradient: a dark diagonal field with soft lights screened
 * over it. Positions are fractions of the canvas, so it looks the same on
 * every monitor size.
 */
function paintGradient(ctx: CanvasRenderingContext2D, w: number, h: number, gradient: GlassGradient) {
  const base = ctx.createLinearGradient(0, 0, w, h);
  base.addColorStop(0, gradient.base[0]);
  base.addColorStop(0.55, gradient.base[1]);
  base.addColorStop(1, gradient.base[2]);
  ctx.fillStyle = base;
  ctx.fillRect(0, 0, w, h);
  ctx.globalCompositeOperation = "screen";
  for (const [x, y, r, color] of gradient.glows) {
    const g = ctx.createRadialGradient(x * w, y * h, 0, x * w, y * h, r * Math.max(w, h));
    g.addColorStop(0, color);
    g.addColorStop(1, "rgba(0,0,0,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
  }
  ctx.globalCompositeOperation = "source-over";
  ctx.filter = `blur(${BLUR_CSS_PX / DOWNSCALE}px)`;
  ctx.drawImage(ctx.canvas, 0, 0);
  ctx.filter = "none";
}

const previews = new Map<string, string>();

/**
 * A small rendering of a Pill gradient for pickers, painted once per id.
 *
 * @param id - Gradient id.
 * @returns A PNG data URL, or `""` where canvas isn't available.
 */
export function gradientPreview(id: string): string {
  const cached = previews.get(id);
  if (cached !== undefined) return cached;
  const made = canvas(192, 120);
  if (!made) return "";
  const [c, ctx] = made;
  paintGradient(ctx, c.width, c.height, glassGradient(id));
  const url = c.toDataURL("image/png");
  previews.set(id, url);
  return url;
}

/**
 * What to paint for a sample under the configured source.
 *
 * - `sample`: blur the sample.
 * - `gradient`: paint the Pill gradient.
 * - `stale`: the sample is from before a source change (the desktop while
 *   a picture is picked, or the other way round); keep what's on screen
 *   until the core sends the right one, so switching never flashes.
 *
 * @param b - Monitor sample from the core, or `null`.
 * @param source - Configured glass source.
 * @param image - Configured picture path.
 * @returns The plan.
 */
export function glassPlan(b: Backdrop | null, source: GlassSource, image: string | null): "sample" | "gradient" | "stale" {
  if (source === "pill") return "gradient";
  const usable = !!b && b.width > 0 && b.height > 0 && b.pixels.length >= b.width * b.height * 3;
  if (!usable) return "gradient";
  // NOTE: the core blurs the desktop for the image source until a picture
  // is set, so a missing path means "desktop" here too.
  const want = source === "image" ? image : null;
  if ((b.image ?? null) !== want) return "stale";
  if (source !== "auto") return "sample";
  return wallpaperCarriesGlass(b) ? "sample" : "gradient";
}

/**
 * Builds the glass image for a monitor.
 *
 * @param b - Monitor sample from the core, or `null`.
 * @param source - Configured glass source.
 * @param gradient - Gradient id for the `pill` source and `auto`'s fallback.
 * @param image - Configured picture path.
 * @returns The blurred backdrop, or `null` if nothing can be painted.
 */
export function toWallpaper(b: Backdrop | null, source: GlassSource, gradient: string, image: string | null = null): Wallpaper | null {
  const monitor: [number, number, number, number] = b?.monitor ?? [0, 0, screen.width * devicePixelRatio, screen.height * devicePixelRatio];
  const scale = b?.scale ?? devicePixelRatio;
  const useSample = !!b && glassPlan(b, source, image) === "sample";
  const w = monitor[2] / scale / DOWNSCALE;
  const h = monitor[3] / scale / DOWNSCALE;
  const made = canvas(w, h);
  if (!made) return null;
  const [c, ctx] = made;
  if (useSample) {
    if (!paintSample(ctx, c.width, c.height, b)) return null;
  } else {
    paintGradient(ctx, c.width, c.height, glassGradient(gradient));
  }
  const painted = useSample ? (b.image ? "image" : "wallpaper") : "pill";
  return { url: c.toDataURL("image/png"), monitor, scale, source: painted };
}

interface GlassKey {
  backdrop: Backdrop | null;
  source: GlassSource;
  gradient: string;
  image: string | null;
}

// PERF: the notch and Settings previews both ask for the image; painting it
// once per sample keeps a theme switch or a re-render from re-blurring.
let last: (GlassKey & { result: Wallpaper | null }) | null = null;

function cachedWallpaper(key: GlassKey): Wallpaper | null {
  if (last && last.backdrop === key.backdrop && last.source === key.source && last.gradient === key.gradient && last.image === key.image) {
    return last.result;
  }
  if (last?.result && glassPlan(key.backdrop, key.source, key.image) === "stale") return last.result;
  const result = toWallpaper(key.backdrop, key.source, key.gradient, key.image);
  last = { ...key, result };
  return result;
}

/**
 * The glass image for this window, rebuilt when the sample or the
 * configured source changes.
 *
 * @returns The wallpaper, or `null` before anything is available.
 */
export function useWallpaper(): Wallpaper | null {
  const backdrop = usePillStore((s) => s.backdrop);
  const source = usePillStore((s) => s.config.glassSource);
  const gradient = usePillStore((s) => s.config.glassGradient);
  const image = usePillStore((s) => s.config.glassImage);
  const theme = usePillStore((s) => s.config.theme);
  return useMemo(
    () => (theme === "glass" ? cachedWallpaper({ backdrop, source, gradient, image }) : null),
    [backdrop, source, gradient, image, theme],
  );
}
