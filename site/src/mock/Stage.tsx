/**
 * The desktop the mockups sit on: a slice of a monitor with one of Pill's
 * glass backdrops, and the notch hanging from its top edge.
 *
 * - {@link Scaled} draws fixed-size app UI at 1:1 and scales it down to fit
 *   narrow screens, so the mockups keep the app's real pixel sizes.
 * - {@link Notch} paints the notch outline the way the app's Pill.tsx does:
 *   one SVG path for the shadow, the glass inside the same path, and a
 *   hairline edge on top.
 * - {@link useMorph} springs the outline between shapes like the app.
 */

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { notchPath, SHAPES, type NotchGeometry, type ShapeMode } from "../lib/shapes";

/**
 * The app's "Graphite" glass backdrop, rebuilt in CSS from its gradient
 * definition in src/lib/backdrop.ts (a diagonal base with screened lights).
 */
const WALLPAPER: CSSProperties = {
  backgroundColor: "#18191d",
  backgroundImage: [
    "radial-gradient(circle at 70% 60%, rgba(148, 163, 184, 0.34), transparent 55%)",
    "radial-gradient(circle at 92% 95%, rgba(71, 85, 105, 0.6), transparent 35%)",
    "radial-gradient(circle at 22% 20%, rgba(226, 232, 240, 0.16), transparent 42%)",
    "radial-gradient(circle at 15% 88%, rgba(100, 116, 139, 0.3), transparent 38%)",
    "linear-gradient(135deg, #2b2e34, #222429 55%, #18191d)",
  ].join(", "),
  backgroundBlendMode: "screen, screen, screen, screen, normal",
};

/**
 * Whether the visitor asked for reduced motion. Follows changes live.
 *
 * @returns `true` when animations should be skipped.
 */
export function useReducedMotion(): boolean {
  const query = "(prefers-reduced-motion: reduce)";
  const [reduced, setReduced] = useState(() => typeof window !== "undefined" && window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

/** Props for {@link Scaled}. */
export interface ScaledProps {
  /** Smallest width the content is laid out at; narrower containers scale it down. */
  minWidth: number;
  /** Widest the content grows to fill its container. */
  maxWidth: number;
  height: number;
  /** Renders the content for the width it is laid out at. */
  children: (width: number) => ReactNode;
  className?: string;
}

/**
 * Lays out fixed-size UI at its real pixel size and scales it down when the
 * container is narrower than `minWidth`.
 *
 * @example
 * <Scaled minWidth={760} maxWidth={1040} height={500}>{(w) => <Desktop width={w} height={500} />}</Scaled>
 */
export function Scaled({ minWidth, maxWidth, height, children, className = "" }: ScaledProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [available, setAvailable] = useState(maxWidth);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setAvailable(el.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const width = Math.max(minWidth, Math.min(maxWidth, available));
  const scale = Math.min(1, available / width);
  return (
    <div ref={ref} className={`relative w-full ${className}`} style={{ height: height * scale }}>
      <div className="absolute top-0 left-1/2 origin-top" style={{ width, height, transform: `translateX(-50%) scale(${scale})` }}>
        {children(width)}
      </div>
    </div>
  );
}

/** Props for {@link Desktop}. */
export interface DesktopProps {
  width: number;
  height: number;
  children: ReactNode;
}

/**
 * A slice of a monitor's top edge with a glass backdrop, framed like a
 * screen. The notch is centred on its top edge.
 */
export function Desktop({ width, height, children }: DesktopProps) {
  return (
    <div
      className="relative overflow-hidden rounded-[20px] ring-1 ring-white/[0.08] shadow-[0_30px_80px_-20px_rgb(0_0_0/0.7)]"
      style={{ width, height, ...WALLPAPER }}
    >
      {children}
    </div>
  );
}

/** Props for {@link Notch}. */
export interface NotchProps {
  geometry: NotchGeometry;
  /** Width of the desktop the notch is centred in. */
  stageWidth: number;
  children: ReactNode;
}

/**
 * The notch outline on the glass theme with its content clipped to it.
 * Glass is the blurred backdrop under the theme tint, so the backdrop is
 * painted again inside the outline rather than sampled with backdrop-filter,
 * which the shadow path underneath would show through.
 */
export function Notch({ geometry, stageWidth, children }: NotchProps) {
  const outer = geometry.width + geometry.shoulder * 2;
  const d = notchPath(geometry, (stageWidth - outer) / 2);
  const clip = `path("${d}")`;
  return (
    <>
      <svg className="pointer-events-none absolute inset-0 size-full overflow-visible drop-shadow-notch" aria-hidden>
        <path d={d} fill="var(--notch-fill)" />
      </svg>
      <div className="absolute inset-0" style={{ clipPath: clip }}>
        <span className="absolute -inset-10 blur-[22px]" style={WALLPAPER} />
        <span className="absolute inset-0" style={{ background: "var(--glass-tint)" }} />
        {children}
      </div>
      <svg className="pointer-events-none absolute inset-0 size-full overflow-visible" aria-hidden>
        <path d={d} fill="none" stroke="var(--notch-edge)" strokeWidth={1} />
      </svg>
    </>
  );
}

const KEYS = ["width", "height", "radius", "shoulder", "shoulderDepth"] as const;

/**
 * Springs the notch outline toward `mode`, like the app's spring (0.38 s,
 * slight bounce). With reduced motion the shape snaps.
 *
 * @param mode - Shape to move to.
 * @param reduced - Skip the animation.
 * @returns The outline for the current frame.
 */
export function useMorph(mode: ShapeMode, reduced: boolean): NotchGeometry {
  const target = SHAPES[mode];
  const [geometry, setGeometry] = useState<NotchGeometry>(() => ({ ...target }));
  const state = useRef({
    value: { ...target } as NotchGeometry,
    velocity: { width: 0, height: 0, radius: 0, shoulder: 0, shoulderDepth: 0 },
  });

  useEffect(() => {
    if (reduced) {
      state.current.value = { ...target };
      setGeometry({ ...target });
      return;
    }
    // A mass-spring with damping ratio ~0.75 settles in about 0.4 s with a
    // small overshoot, close to the app's framer-motion spring.
    const stiffness = 260;
    const damping = 24;
    let frame = 0;
    let last = performance.now();
    const step = (now: number) => {
      const dt = Math.min(0.032, (now - last) / 1000);
      last = now;
      const { value, velocity } = state.current;
      let moving = false;
      for (const k of KEYS) {
        const force = stiffness * (target[k] - value[k]) - damping * velocity[k];
        velocity[k] += force * dt;
        value[k] += velocity[k] * dt;
        if (Math.abs(target[k] - value[k]) > 0.05 || Math.abs(velocity[k]) > 0.05) moving = true;
      }
      if (!moving) Object.assign(value, target);
      setGeometry({ ...value });
      if (moving) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [target, reduced]);

  return geometry;
}
