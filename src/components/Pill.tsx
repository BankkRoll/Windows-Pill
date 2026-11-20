import {
  animate,
  AnimatePresence,
  motion,
  useMotionValue,
  useReducedMotion,
  useTransform,
  type MotionValue,
  type Transition,
} from "framer-motion";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useHoverIntent } from "../hooks/useHoverIntent";
import { EVENTS, isTauri, releaseFocus, setHitRect, subscribe } from "../lib/native";
import { usePalette } from "../lib/theme";
import {
  contentGroup,
  grows,
  notchPath,
  SHAPES,
  shapeRect,
  type NotchGeometry,
  type ShapeMode,
} from "../lib/shapes";
import { restingShape, usePillStore } from "../store";
import { CollapsedContent } from "./CollapsedContent";
import { ExpandedPanel } from "./ExpandedPanel";
import { GlassBackground } from "./Glass";
import { CommandBar } from "./CommandBar";

const MORPH: Transition = { type: "spring", duration: 0.38, bounce: 0.18 };
const NO_MORPH: Transition = { duration: 0 };
const FADE_OUT = 0.08;
const GEOMETRY_KEYS = ["width", "height", "radius", "shoulder", "shoulderDepth"] as const;

/**
 * The notch hanging from the top of the screen and everything inside it.
 *
 * The outline is one SVG path whose parameters spring together, so the
 * shoulders, sides and corners morph as a single continuous shape. Content is
 * clipped to the same path. Opening grows the shape immediately and fades
 * content in shortly after; closing fades content out first, then shrinks,
 * so text is never squashed. With Windows animation effects off (reported by
 * WebView2 as `prefers-reduced-motion`), the shape snaps and only fades remain.
 */
export function Pill() {
  const config = usePillStore((s) => s.config);
  const signals = usePillStore((s) => s.signals);
  const hasLive = usePillStore((s) => s.plugins.some((p) => p.enabled && p.live !== null));
  const held = usePillStore((s) => s.holds.size > 0);
  const reduceMotion = useReducedMotion() ?? false;

  const [nativeHover, setNativeHover] = useState(false);
  const [domHover, setDomHover] = useState(false);
  useEffect(() => subscribe<boolean>(EVENTS.hover, setNativeHover), []);
  const stepAside = usePillStore((s) => s.stepAside);
  const collapseSeq = usePillStore((s) => s.collapseSeq);
  const rawHover = isTauri ? nativeHover : domHover;
  // After opening something outside Pill the cursor is usually still on the
  // notch; it must leave once before hovering can open the panel again.
  const [waitForLeave, setWaitForLeave] = useState(false);
  useEffect(() => {
    if (!rawHover) setWaitForLeave(false);
  }, [rawHover]);
  const hovered = rawHover && !stepAside && !waitForLeave;

  const [open, setOpen] = useHoverIntent(
    hovered,
    { openDelay: config.hoverOpenDelayMs, closeDelay: config.hoverCloseDelayMs },
    held && !stepAside,
    config.openOn === "hover",
  );

  // A window drag toward the top of the screen opens Windows' snap bar right
  // under the notch; close the panel so nothing covers it.
  useEffect(() => {
    if (stepAside) setOpen(false);
  }, [stepAside, setOpen]);

  useEffect(() => {
    if (collapseSeq === 0) return;
    setWaitForLeave(true);
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    setOpen(false);
    void releaseFocus();
  }, [collapseSeq, setOpen]);

  const quickAdd = usePillStore((s) => s.quickAdd);
  const setQuickAdd = usePillStore((s) => s.setQuickAdd);
  const closeQuickAdd = () => {
    setQuickAdd(false);
    void releaseFocus();
  };

  // Clicking into another app deactivates the window; quick-add closes with it.
  useEffect(() => {
    if (!quickAdd) return;
    const onBlur = () => setQuickAdd(false);
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, [quickAdd, setQuickAdd]);

  // Quick-add wins over hover so the input never jumps away mid-typing.
  const commandTall = usePillStore((s) => s.commandTall);
  const view: ShapeMode = quickAdd ? (commandTall ? "command" : "quickAdd") : open || held ? "expanded" : restingShape(signals, hasLive);
  const [shape, setShape] = useState<ShapeMode>(view);
  const viewRef = useRef(view);
  viewRef.current = view;

  useEffect(() => {
    if (view !== shape && (grows(shape, view) || contentGroup(shape) === contentGroup(view))) {
      setShape(view);
    }
  }, [view, shape]);

  useEffect(() => {
    const report = () => setHitRect(shapeRect(shape, window.innerWidth));
    report();
    window.addEventListener("resize", report);
    return () => window.removeEventListener("resize", report);
  }, [shape]);

  const { path, clip } = useNotchOutline(shape, reduceMotion ? NO_MORPH : MORPH);
  const group = contentGroup(view);
  const growing = group !== "collapsed";

  return (
    <motion.div
      className="pointer-events-none fixed inset-0"
      animate={{ opacity: stepAside ? 0 : 1 }}
      transition={{ duration: stepAside ? 0.12 : 0.2 }}
    >
      <NotchSurface path={path} clip={clip} />
      <motion.div
        className="pointer-events-auto absolute inset-0"
        style={{ clipPath: clip }}
        onMouseEnter={() => setDomHover(true)}
        onMouseLeave={() => setDomHover(false)}
        onClick={() => {
          if (!open && !quickAdd) setOpen(true);
        }}
      >
        <AnimatePresence mode="wait" initial={false} onExitComplete={() => setShape(viewRef.current)}>
          <Fade key={group} delay={growing ? 0.02 : 0.1}>
            {group === "collapsed" ? (
              <CollapsedContent width={SHAPES[view].width} height={SHAPES[view].height} />
            ) : group === "quickAdd" ? (
              <div style={{ height: SHAPES[view].height }}>
                <CommandBar variant="notch" onClose={closeQuickAdd} />
              </div>
            ) : (
              <ExpandedPanel />
            )}
          </Fade>
        </AnimatePresence>
      </motion.div>
    </motion.div>
  );
}

/**
 * Springs every outline parameter toward `mode` and derives the SVG path and
 * matching CSS clip-path for each frame.
 */
function useNotchOutline(mode: ShapeMode, transition: Transition) {
  const target = SHAPES[mode];
  const values = {
    width: useMotionValue(target.width),
    height: useMotionValue(target.height),
    radius: useMotionValue(target.radius),
    shoulder: useMotionValue(target.shoulder),
    shoulderDepth: useMotionValue(target.shoulderDepth),
  } satisfies Record<keyof NotchGeometry, MotionValue<number>>;
  const viewport = useMotionValue(typeof window === "undefined" ? 0 : window.innerWidth);

  useEffect(() => {
    const onResize = () => viewport.set(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [viewport]);

  useEffect(() => {
    const controls = GEOMETRY_KEYS.map((k) => animate(values[k], target[k], transition));
    return () => controls.forEach((c) => c.stop());
    // eslint-disable-next-line react-hooks/exhaustive-deps -- motion values are stable
  }, [mode, transition]);

  const path = useTransform(() => {
    const g: NotchGeometry = {
      width: values.width.get(),
      height: values.height.get(),
      radius: values.radius.get(),
      shoulder: values.shoulder.get(),
      shoulderDepth: values.shoulderDepth.get(),
    };
    const outer = g.width + g.shoulder * 2;
    return notchPath(g, (viewport.get() - outer) / 2);
  });
  const clip = useTransform(() => `path("${path.get()}")`);
  return { path, clip };
}

/**
 * Paints the notch for the current theme. The filled path underneath casts
 * the drop shadow; for glass, the wallpaper layer sits on top clipped to the
 * same outline, and a hairline edge goes over both.
 */
function NotchSurface({ path, clip }: { path: MotionValue<string>; clip: MotionValue<string> }) {
  const theme = usePillStore((s) => s.config.theme);
  const palette = usePalette();
  return (
    <>
      <svg className="absolute inset-0 size-full overflow-visible drop-shadow-notch" aria-hidden>
        <motion.path d={path} fill="var(--notch-fill)" />
      </svg>
      {theme === "glass" && <GlassBackground clip={clip} />}
      {(theme !== "black" || palette === "light") && (
        <svg className="pointer-events-none absolute inset-0 size-full overflow-visible" aria-hidden>
          <motion.path d={path} fill="none" stroke="var(--notch-edge)" strokeWidth={1} />
        </svg>
      )}
    </>
  );
}

function Fade({ children, delay }: { children: ReactNode; delay: number }) {
  return (
    <motion.div
      className="absolute top-0 left-1/2 -translate-x-1/2"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { duration: 0.14, delay } }}
      exit={{ opacity: 0, transition: { duration: FADE_OUT } }}
    >
      {children}
    </motion.div>
  );
}
