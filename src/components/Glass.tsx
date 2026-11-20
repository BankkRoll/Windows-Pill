/**
 * The glass material shared by the notch and the detached window: this
 * window's slice of the blurred monitor wallpaper under the theme tint.
 * One implementation keeps both surfaces identical.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";
import { motion, type MotionValue } from "framer-motion";
import { useEffect, useRef } from "react";
import { isTauri } from "../lib/native";
import { useWallpaper } from "../lib/backdrop";
import { usePalette, type Palette } from "../lib/theme";

/**
 * The tint layers that go over a wallpaper image to make it glass: a
 * luminosity layer (only painted in light, where it lifts dark wallpapers
 * without losing their hue) and the palette's tint. Fills its positioned
 * parent; the Settings previews reuse it so they match the real surface.
 *
 * @param props.palette - Palette being previewed; defaults to the window's.
 */
export function GlassTintLayer({ palette }: { palette?: Palette }) {
  const current = usePalette();
  return (
    <>
      {/* PERF: a blend layer makes the compositor flatten the whole clip
          group, so dark, where it would be a no-op, leaves it out. */}
      {(palette ?? current) === "light" && (
        <span className="absolute inset-0" style={{ background: "var(--glass-luminosity)", mixBlendMode: "luminosity" }} />
      )}
      <span className="absolute inset-0" style={{ background: "var(--glass-tint)" }} />
    </>
  );
}

/**
 * Fills its positioned parent with glass. The wallpaper layer is offset by
 * the window's position on its monitor and follows moves by updating a
 * transform directly, so a drag never waits on React or the core.
 *
 * @param props.clip - Optional CSS `clip-path` (the notch outline).
 */
export function GlassBackground({ clip }: { clip?: MotionValue<string> }) {
  const wallpaper = useWallpaper();
  const layer = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!wallpaper || !isTauri) return;
    const [mx, my] = wallpaper.monitor;
    const place = (x: number, y: number) => {
      const el = layer.current;
      if (el) el.style.transform = `translate3d(${(mx - x) / wallpaper.scale}px, ${(my - y) / wallpaper.scale}px, 0)`;
    };
    const win = getCurrentWindow();
    let alive = true;
    void win.innerPosition().then((p) => alive && place(p.x, p.y));
    let frame = 0;
    // NOTE: Move events carry the outer frame's position, which sits outside
    // the client area by the invisible resize border (9 px at 125 %). The
    // glass must line up with the client area, so take the frame's inset once
    // and apply it to every move.
    let inset: { x: number; y: number } | null = null;
    void Promise.all([win.innerPosition(), win.outerPosition()]).then(([i, o]) => {
      inset = { x: i.x - o.x, y: i.y - o.y };
    });
    const moved = win.onMoved(({ payload }) => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (inset) place(payload.x + inset.x, payload.y + inset.y);
        else void win.innerPosition().then((p) => place(p.x, p.y));
      });
    });
    return () => {
      alive = false;
      cancelAnimationFrame(frame);
      void moved.then((f) => f());
    };
  }, [wallpaper]);

  if (!wallpaper) return null;
  const [, , mw, mh] = wallpaper.monitor;
  return (
    <motion.div className="pointer-events-none absolute inset-0 overflow-hidden" style={{ clipPath: clip }} aria-hidden>
      <div
        ref={layer}
        className="absolute top-0 left-0 will-change-transform"
        style={{
          width: mw / wallpaper.scale,
          height: mh / wallpaper.scale,
          backgroundImage: `url(${wallpaper.url})`,
          backgroundSize: "100% 100%",
        }}
      />
      <GlassTintLayer />
    </motion.div>
  );
}
