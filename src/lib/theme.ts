/**
 * Light and dark palettes on top of the surface styles.
 *
 * The look has two independent axes: the surface style (`theme`: glass,
 * graphite or black) and the palette (`appearance`: dark, light or
 * following Windows). The stylesheet keys every token off `data-theme`,
 * `data-appearance` and `data-tint` on the same element, so a subtree such
 * as a Settings preview tile can show another combination than the page.
 *
 * Responsibilities:
 * - Resolve the `system` appearance to a concrete palette.
 * - Darken the accent where it would be too faint on light surfaces.
 * - Name each surface style the way it looks in each palette.
 * - Mirror the choice onto an element's `data-*` attributes.
 */

import { usePillStore } from "../store";
import type { Appearance, GlassTint, Theme } from "./native";

/** A concrete palette, after `system` is resolved. */
export type Palette = "dark" | "light";

/**
 * Resolves the configured appearance.
 *
 * @param appearance - Configured appearance.
 * @param systemLight - Whether Windows asks apps for light mode.
 * @returns The palette to paint.
 * @example resolvePalette("system", true) // "light"
 */
export function resolvePalette(appearance: Appearance, systemLight: boolean): Palette {
  if (appearance === "system") return systemLight ? "light" : "dark";
  return appearance;
}

/**
 * The palette this window paints right now.
 *
 * @returns `"light"` or `"dark"`, following Windows for `system`.
 */
export function usePalette(): Palette {
  const appearance = usePillStore((s) => s.config.appearance);
  const systemLight = usePillStore((s) => s.systemLight);
  return resolvePalette(appearance, systemLight);
}

function channels(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = Number.parseInt(m[1]!, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function luminance([r, g, b]: [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/**
 * WCAG contrast ratio between two opaque `#RRGGBB` colours.
 *
 * @param a - First colour.
 * @param b - Second colour.
 * @returns A ratio from 1 to 21, or 1 when either colour can't be read.
 * @example contrastRatio("#000000", "#ffffff") // 21
 */
export function contrastRatio(a: string, b: string): number {
  const ca = channels(a);
  const cb = channels(b);
  if (!ca || !cb) return 1;
  const [hi, lo] = [luminance(ca), luminance(cb)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * The darkest base of the light surfaces (light glass with no wallpaper
 * sample yet). Accent text that reads on it reads on all of them.
 */
const LIGHT_BASE = "#ececef";

/** WCAG AA for body text. */
const AA = 4.5;

/**
 * The accent to paint in a palette. Dark keeps the user's colour exactly;
 * light darkens it toward black, keeping its hue, until it passes AA as text
 * on the light surfaces. White labels on the accent then pass too.
 *
 * @param accent - Configured accent as `#RRGGBB`.
 * @param palette - Palette being painted.
 * @returns A `#rrggbb` colour, or `accent` unchanged.
 * @example readableAccent("#FF9F0A", "light") // a deep orange that passes 4.5:1
 */
export function readableAccent(accent: string, palette: Palette): string {
  const rgb = channels(accent);
  if (palette === "dark" || !rgb) return accent;
  const hex = (c: [number, number, number]) => `#${c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
  for (let keep = 1; keep > 0; keep -= 0.02) {
    const mixed = rgb.map((v) => v * keep) as [number, number, number];
    if (contrastRatio(hex(mixed), LIGHT_BASE) >= AA) return keep === 1 ? accent : hex(mixed);
  }
  return "#000000";
}

/** How each surface style is named and described in each palette. */
export const SURFACES: Readonly<Record<Theme, Readonly<Record<Palette, { label: string; hint: string }>>>> = {
  glass: {
    dark: { label: "Glass", hint: "Your wallpaper, blurred" },
    light: { label: "Glass", hint: "Your wallpaper, blurred" },
  },
  graphite: {
    dark: { label: "Graphite", hint: "Soft dark gray" },
    light: { label: "Silver", hint: "Soft light gray" },
  },
  black: {
    dark: { label: "Black", hint: "Like a hardware notch" },
    light: { label: "White", hint: "Clean and bright" },
  },
};

/** What paints an element: surface style, palette and glass tint. */
export interface PaletteChoice {
  theme: Theme;
  palette: Palette;
  glassTint: GlassTint;
}

/**
 * The `data-*` attributes the stylesheet reads, for JSX.
 *
 * @param choice - Surface style, palette and tint.
 * @returns Attributes to spread onto an element.
 * @example <span {...paletteAttributes({ theme: "black", palette: "light", glassTint: "medium" })} />
 */
export function paletteAttributes(choice: PaletteChoice): Record<"data-theme" | "data-appearance" | "data-tint", string> {
  return { "data-theme": choice.theme, "data-appearance": choice.palette, "data-tint": choice.glassTint };
}

/**
 * Mirrors the palette onto an element, normally `<html>`, along with the
 * accent the palette calls for.
 *
 * @param el - Element to style.
 * @param choice - Surface style, palette and tint.
 * @param accent - Configured accent as `#RRGGBB`.
 */
export function applyPalette(el: HTMLElement, choice: PaletteChoice, accent: string) {
  el.dataset.theme = choice.theme;
  el.dataset.appearance = choice.palette;
  el.dataset.tint = choice.glassTint;
  el.style.setProperty("--color-accent", readableAccent(accent, choice.palette));
}
