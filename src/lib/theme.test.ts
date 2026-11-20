import { describe, expect, it } from "vitest";
import { contrastRatio, paletteAttributes, readableAccent, resolvePalette, SURFACES } from "./theme";

// NOTE: Vitest hands CSS imports to tests as empty strings, even with ?raw,
// and the project has no Node typings, so the stylesheet is read through a
// loosely typed fs import instead.
const fs = (await import(/* @vite-ignore */ `node:${"fs"}`)) as { readFileSync(path: URL, encoding: "utf8"): string };
const css = fs.readFileSync(new URL("../index.css", import.meta.url), "utf8");

/** The `#rrggbb` a declaration in the first rule matching `selector` sets. */
function token(selector: string, name: string): string {
  const start = css.indexOf(`${selector} {`);
  expect(start, selector).toBeGreaterThanOrEqual(0);
  const body = css.slice(start, css.indexOf("}", start));
  const m = new RegExp(`${name}:\\s*(#[0-9a-f]{3,6})`, "i").exec(body);
  expect(m, `${selector} ${name}`).not.toBeNull();
  const hex = m![1]!;
  return hex.length === 4 ? `#${[...hex.slice(1)].map((c) => c + c).join("")}` : hex;
}

/** `fg` at `alpha` over the opaque `bg`, as `#rrggbb`. */
function over(fg: string, alpha: number, bg: string): string {
  const ch = (h: string) => [1, 3, 5].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
  const [f, b] = [ch(fg), ch(bg)];
  return `#${f.map((v, i) => Math.round(v * alpha + b[i]! * (1 - alpha)).toString(16).padStart(2, "0")).join("")}`;
}

const LIGHT_SURFACES = ["#ffffff", "#f2f2f4", "#ececef"];
const ACCENTS = ["#2F7BFF", "#0A84FF", "#5E5CE6", "#BF5AF2", "#FF375F", "#FF9F0A", "#30D158", "#64D2FF"];

describe("resolvePalette", () => {
  it("follows Windows only for system", () => {
    expect(resolvePalette("dark", true)).toBe("dark");
    expect(resolvePalette("light", false)).toBe("light");
    expect(resolvePalette("system", true)).toBe("light");
    expect(resolvePalette("system", false)).toBe("dark");
  });
});

describe("contrastRatio", () => {
  it("matches the WCAG extremes and ignores order", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 5);
    expect(contrastRatio("#777777", "#777777")).toBe(1);
    expect(contrastRatio("nope", "#ffffff")).toBe(1);
  });
});

describe("readableAccent", () => {
  it("leaves dark exactly as configured", () => {
    for (const a of ACCENTS) expect(readableAccent(a, "dark")).toBe(a);
  });

  it("makes every preset pass AA in light, as text and under white labels", () => {
    for (const a of ACCENTS) {
      const light = readableAccent(a, "light");
      for (const bg of LIGHT_SURFACES) expect(contrastRatio(light, bg), `${a} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(light, "#ffffff"), `white on ${a}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps an accent that already passes and darkens only as far as needed", () => {
    expect(readableAccent("#1a3d8f", "light")).toBe("#1a3d8f");
    const blue = readableAccent("#2F7BFF", "light");
    expect(blue).not.toBe("#2F7BFF");
    expect(contrastRatio(blue, "#ececef")).toBeLessThan(5.2);
    expect(readableAccent("blue", "light")).toBe("blue");
  });
});

describe("light palette tokens", () => {
  const ink = token(`[data-appearance="light"]`, "--color-white");
  const fills = ["black", "graphite", "glass"].map((t) => token(`[data-theme="${t}"][data-appearance="light"]`, "--window-fill"));

  it("paints the surfaces Rust fills the pop-out with", () => {
    expect(fills).toEqual(LIGHT_SURFACES);
  });

  it("keeps body text at AA on every light surface", () => {
    for (const bg of fills) {
      expect(contrastRatio(over(ink, 0.7, bg), bg)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps status colours at AA as text on every light surface", () => {
    for (const status of ["green", "amber", "red", "purple"]) {
      const color = token(`[data-appearance="light"]`, `--color-status-${status}`);
      for (const bg of fills) expect(contrastRatio(color, bg), `${status} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      expect(contrastRatio(color, "#ffffff"), `white on ${status}`).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("SURFACES and paletteAttributes", () => {
  it("names every style in both palettes", () => {
    expect(SURFACES.black.light.label).toBe("White");
    expect(SURFACES.graphite.dark.label).toBe("Graphite");
    for (const style of Object.values(SURFACES)) {
      expect(style.dark.label && style.light.label && style.dark.hint && style.light.hint).toBeTruthy();
    }
  });

  it("emits the attributes the stylesheet keys on", () => {
    expect(paletteAttributes({ theme: "glass", palette: "light", glassTint: "dark" })).toEqual({
      "data-theme": "glass",
      "data-appearance": "light",
      "data-tint": "dark",
    });
  });
});
