/**
 * Geometry of the notch, copied from the app's src/lib/shapes.ts so the
 * mockups draw the same outline at the same sizes.
 *
 * The notch hangs from the top edge of the monitor: its top corners are
 * square and flare into the screen edge with concave fillets ("ears"), and
 * only its bottom corners are rounded.
 */

/** Visual states of the shape. The fully hidden state is handled natively. */
export type ShapeMode = "idle" | "peek" | "expanded" | "quickAdd" | "command";

/** Size and curve parameters of one shape state, in CSS pixels. */
export interface ShapeSpec {
  /** Width of the body between the shoulders (the content area). */
  width: number;
  height: number;
  /** Radius of the two bottom corners. */
  radius: number;
  /** Horizontal reach of each shoulder flaring into the screen edge. */
  shoulder: number;
  /** How far down the shoulder curve runs before the side turns vertical. */
  shoulderDepth: number;
}

/** Dimensions for every state (at 100% scaling). */
export const SHAPES: Readonly<Record<ShapeMode, ShapeSpec>> = {
  idle: { width: 200, height: 30, radius: 14, shoulder: 26, shoulderDepth: 16 },
  peek: { width: 244, height: 34, radius: 16, shoulder: 28, shoulderDepth: 18 },
  expanded: { width: 680, height: 440, radius: 30, shoulder: 30, shoulderDepth: 22 },
  quickAdd: { width: 600, height: 54, radius: 20, shoulder: 28, shoulderDepth: 20 },
  /** Quick-add with its suggestions or an AI answer showing. */
  command: { width: 600, height: 360, radius: 26, shoulder: 30, shoulderDepth: 22 },
};

/** Numbers that fully describe one frame of the notch outline. */
export interface NotchGeometry {
  width: number;
  height: number;
  radius: number;
  shoulder: number;
  shoulderDepth: number;
}

// Bezier handle length for a quarter circle (1 - 0.5523), so corners read as true arcs.
const ARC = 0.4477;

/**
 * SVG path for the notch: the screen edge flows into an S-shaped shoulder,
 * down a side, around a rounded bottom corner, along a flat bottom and back.
 * Shoulders leave the top edge horizontally, so the notch blends into the
 * bezel with no visible seam or nub.
 *
 * @param g - Outline parameters (any frame of a morph).
 * @param x0 - Left edge of the outline (the start of the left shoulder).
 * @returns An SVG path `d` string.
 * @example
 * notchPath({ width: 200, height: 30, radius: 14, shoulder: 14, shoulderDepth: 16 }, 0);
 */
export function notchPath(g: NotchGeometry, x0: number): string {
  const { width: w, height: h, shoulder: s } = g;
  const r = Math.max(0, Math.min(g.radius, h / 2, w / 2));
  const e = Math.max(0, Math.min(g.shoulderDepth, h - r));
  const L = x0 + s; // left side of the body
  const R = L + w; // right side of the body
  const f = (n: number) => Math.round(n * 100) / 100;
  return [
    `M${f(x0)},0`,
    `C${f(x0 + s * 0.62)},0 ${f(L)},${f(e * 0.38)} ${f(L)},${f(e)}`,
    `L${f(L)},${f(h - r)}`,
    `C${f(L)},${f(h - r * ARC)} ${f(L + r * ARC)},${f(h)} ${f(L + r)},${f(h)}`,
    `L${f(R - r)},${f(h)}`,
    `C${f(R - r * ARC)},${f(h)} ${f(R)},${f(h - r * ARC)} ${f(R)},${f(h - r)}`,
    `L${f(R)},${f(e)}`,
    `C${f(R)},${f(e * 0.38)} ${f(R + s * 0.38)},0 ${f(R + s)},0`,
    "Z",
  ].join(" ");
}
