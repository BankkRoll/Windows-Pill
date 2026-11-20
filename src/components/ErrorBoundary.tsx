/**
 * Keeps a crash in one view from blanking the whole notch or window.
 *
 * - {@link ErrorBoundary}: catches render errors below it and shows a
 *   fallback with a Reload button.
 * - Fallbacks: `view` fills a tab's area; `notch` draws a small notch of
 *   its own, since a crash in the notch takes its outline with it.
 */
import { Component, useEffect, type ErrorInfo, type ReactNode } from "react";
import { setHitRect } from "../lib/native";
import { shapeRect } from "../lib/shapes";

/** Props for {@link ErrorBoundary}. */
export interface ErrorBoundaryProps {
  /** `view` for a tab or window body; `notch` for the notch's root. */
  fallback?: "view" | "notch";
  children: ReactNode;
}

/**
 * Catches errors thrown while rendering `children` and shows a calm
 * fallback instead of an empty window. Give it a `key` that changes with
 * the view (the tab id, say) so moving elsewhere tries again.
 *
 * @example
 * <ErrorBoundary key={activeTab}><View /></ErrorBoundary>
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("view crashed", error, info.componentStack);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return this.props.fallback === "notch" ? <NotchFallback /> : <ViewFallback />;
  }
}

const reload = () => window.location.reload();

function ReloadButton() {
  return (
    <button
      type="button"
      onClick={reload}
      className="h-7 shrink-0 rounded-lg bg-white/8 px-3 text-[12px] text-white/85 outline-none hover:bg-white/12 focus-visible:ring-1 focus-visible:ring-white/40"
    >
      Reload
    </button>
  );
}

function ViewFallback() {
  return (
    <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 px-10 text-center">
      <div>
        <p className="font-display text-[15px] font-semibold text-white/85">This part of Pill stopped working</p>
        <p className="mt-1.5 max-w-sm text-[12px] leading-relaxed text-white/50">
          Your tasks and settings are safe on disk. Reloading usually sorts it out.
        </p>
      </div>
      <ReloadButton />
    </div>
  );
}

function NotchFallback() {
  const rect = shapeRect("quickAdd", window.innerWidth);
  // The crashed notch can no longer report its shape, and the window is
  // click-through outside it, so this fallback claims its own area.
  useEffect(() => setHitRect(shapeRect("quickAdd", window.innerWidth)), []);

  return (
    <div
      role="alert"
      className="fixed flex items-center gap-3 px-6 text-[12.5px] text-white/75"
      style={{
        left: rect.x,
        top: rect.y,
        width: rect.width,
        height: rect.height,
        borderRadius: `0 0 ${rect.radius}px ${rect.radius}px`,
        background: "var(--notch-fill)",
      }}
    >
      <span className="min-w-0 flex-1 truncate">Pill ran into a problem. Your tasks are safe.</span>
      <ReloadButton />
    </div>
  );
}
