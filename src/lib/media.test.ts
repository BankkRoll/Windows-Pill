import { describe, expect, it } from "vitest";
import { artworkWidth, groupSessions, formatClock, keySeek, positionAt, repeatLabel, sameTimeline, trackFraction } from "./media";
import type { MediaSession, MediaTimeline } from "./native";

const tl = (patch: Partial<MediaTimeline> = {}): MediaTimeline => ({
  positionMs: 60_000,
  durationMs: 180_000,
  updatedAt: 1_000,
  rate: 1,
  ...patch,
});

describe("positionAt", () => {
  it("advances from the last report while playing", () => {
    expect(positionAt(tl(), true, 4_000)).toBe(63_000);
    expect(positionAt(tl({ rate: 2 }), true, 4_000)).toBe(66_000);
  });

  it("holds still while paused", () => {
    expect(positionAt(tl(), false, 90_000)).toBe(60_000);
  });

  it("stays inside the track", () => {
    expect(positionAt(tl(), true, 10_000_000)).toBe(180_000);
    expect(positionAt(tl({ positionMs: -5 }), false, 0)).toBe(0);
    expect(positionAt(tl(), true, 0)).toBe(60_000);
  });
});

describe("formatClock", () => {
  it("formats minutes and hours", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(83_400)).toBe("1:23");
    expect(formatClock(225_000)).toBe("3:45");
    expect(formatClock(3_723_000)).toBe("1:02:03");
  });

  it("treats junk as zero", () => {
    expect(formatClock(-1)).toBe("0:00");
    expect(formatClock(Number.NaN)).toBe("0:00");
  });
});

describe("trackFraction", () => {
  it("clamps to the track", () => {
    expect(trackFraction(150, 100, 200)).toBe(0.25);
    expect(trackFraction(50, 100, 200)).toBe(0);
    expect(trackFraction(400, 100, 200)).toBe(1);
    expect(trackFraction(150, 100, 0)).toBe(0);
  });
});

describe("keySeek", () => {
  it("steps, pages and jumps", () => {
    expect(keySeek("ArrowRight", 60_000, 180_000)).toBe(65_000);
    expect(keySeek("ArrowDown", 60_000, 180_000)).toBe(55_000);
    expect(keySeek("PageUp", 60_000, 180_000)).toBe(78_000);
    expect(keySeek("PageDown", 40_000, 50_000)).toBe(30_000);
    expect(keySeek("Home", 60_000, 180_000)).toBe(0);
    expect(keySeek("End", 60_000, 180_000)).toBe(180_000);
  });

  it("clamps and ignores other keys", () => {
    expect(keySeek("ArrowLeft", 2_000, 180_000)).toBe(0);
    expect(keySeek("ArrowRight", 178_000, 180_000)).toBe(180_000);
    expect(keySeek("Enter", 0, 180_000)).toBeNull();
  });
});

describe("labels and sizes", () => {
  it("names the current repeat mode", () => {
    expect(repeatLabel("none")).toBe("Repeat off");
    expect(repeatLabel("list")).toBe("Repeat all");
    expect(repeatLabel("track")).toBe("Repeat one");
  });

  it("keeps artwork between square and 16:9", () => {
    expect(artworkWidth(1, 48)).toBe(48);
    expect(artworkWidth(1.78, 48)).toBe(85);
    expect(artworkWidth(3, 48)).toBe(85);
    expect(artworkWidth(0.5, 48)).toBe(48);
    expect(artworkWidth(Number.NaN, 48)).toBe(48);
  });

  it("compares timelines by value", () => {
    expect(sameTimeline(tl(), tl())).toBe(true);
    expect(sameTimeline(tl(), tl({ updatedAt: 2 }))).toBe(false);
    expect(sameTimeline(null, null)).toBe(true);
    expect(sameTimeline(tl(), null)).toBe(false);
  });
});

describe("groupSessions", () => {
  const s = (id: number, app: string, title: string, artist = ""): MediaSession => ({ id, app, icon: null, title, artist, playing: true });

  it("groups by app in order of first appearance and keeps every session", () => {
    const groups = groupSessions([s(1, "Chrome", "Stream"), s(2, "Spotify", "Song"), s(3, "Chrome", "FREETA")]);
    expect(groups.map((g) => [g.app, g.entries.map((e) => e.session.id)])).toEqual([
      ["Chrome", [1, 3]],
      ["Spotify", [2]],
    ]);
  });

  it("numbers sessions that repeat an earlier title and artist", () => {
    const [chrome] = groupSessions([s(1, "Chrome", "FREETA", "chan"), s(2, "Chrome", "FREETA", "chan"), s(3, "Chrome", "FREETA", "other")]);
    expect(chrome!.entries.map((e) => e.repeat)).toEqual([null, 2, null]);
  });
});
