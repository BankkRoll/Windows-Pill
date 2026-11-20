import { describe, expect, it } from "vitest";
import { GLASS_GRADIENTS, glassGradient, glassPlan, wallpaperCarriesGlass } from "./backdrop";
import type { Backdrop } from "./native";

function sample(pixels: number[][], image: string | null = null): Backdrop {
  return { width: pixels.length, height: 1, pixels: pixels.flat(), monitor: [0, 0, 100, 100], scale: 1, image };
}

const varied = [[20, 60, 160], [160, 180, 220], [40, 90, 120]];
const black = [[0, 0, 0], [4, 4, 4], [2, 2, 2]];

describe("wallpaperCarriesGlass", () => {
  it("rejects a black wallpaper", () => {
    expect(wallpaperCarriesGlass(sample(black))).toBe(false);
  });

  it("rejects a flat single color", () => {
    expect(wallpaperCarriesGlass(sample([[90, 120, 200], [90, 120, 200], [91, 121, 200]]))).toBe(false);
  });

  it("accepts a varied wallpaper", () => {
    expect(wallpaperCarriesGlass(sample(varied))).toBe(true);
  });
});

describe("glassPlan", () => {
  it("falls back to a gradient on a dark wallpaper only in auto", () => {
    expect(glassPlan(sample(black), "auto", null)).toBe("gradient");
    expect(glassPlan(sample(black), "wallpaper", null)).toBe("sample");
    expect(glassPlan(sample(varied), "auto", null)).toBe("sample");
  });

  it("always paints the gradient for the pill source", () => {
    expect(glassPlan(sample(varied), "pill", null)).toBe("gradient");
  });

  it("paints a gradient when there's no usable sample", () => {
    expect(glassPlan(null, "wallpaper", null)).toBe("gradient");
    expect(glassPlan({ ...sample(varied), pixels: [] }, "image", "C:\\a.png")).toBe("gradient");
  });

  it("uses a picture's sample only once it's the picked one", () => {
    expect(glassPlan(sample(black, "C:\\a.png"), "image", "C:\\a.png")).toBe("sample");
    expect(glassPlan(sample(varied), "image", "C:\\a.png")).toBe("stale");
    expect(glassPlan(sample(varied, "C:\\b.png"), "image", "C:\\a.png")).toBe("stale");
    expect(glassPlan(sample(varied, "C:\\a.png"), "auto", null)).toBe("stale");
  });

  it("treats the image source without a picture as the desktop", () => {
    expect(glassPlan(sample(varied), "image", null)).toBe("sample");
  });
});

describe("glassGradient", () => {
  it("has unique ids and falls back to the first", () => {
    expect(new Set(GLASS_GRADIENTS.map((g) => g.id)).size).toBe(GLASS_GRADIENTS.length);
    expect(glassGradient("ember").label).toBe("Ember");
    expect(glassGradient("nope")).toBe(GLASS_GRADIENTS[0]);
  });
});
