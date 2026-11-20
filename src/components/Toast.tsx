import { AnimatePresence, motion } from "framer-motion";
import { useEffect } from "react";
import { toastTimeLeft } from "../lib/toast";
import { usePillStore } from "../store";

/**
 * The single toast line, with an optional Undo button.
 *
 * @param props.variant - `panel` floats above the panel's bottom bar;
 *   `inline` sits in the quick-add bar, which is too short for it to float.
 */
export function Toast({ variant = "panel" }: { variant?: "panel" | "inline" }) {
  const toast = usePillStore((s) => s.toast);
  const dismiss = usePillStore((s) => s.dismissToast);
  // Checked during render too, so a toast raised while nothing could show it
  // never flashes up when a view mounts later.
  const live = toast !== null && toastTimeLeft(toast, Date.now()) > 0 ? toast : null;

  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => dismiss(toast.id), Math.max(0, toastTimeLeft(toast, Date.now())));
    return () => window.clearTimeout(id);
  }, [toast, dismiss]);

  const undo = live?.undo && (
    <button
      type="button"
      // Keeps the caret in the quick-add input, whose blur would close the bar.
      onMouseDown={variant === "inline" ? (e) => e.preventDefault() : undefined}
      onClick={() => {
        live.undo?.();
        dismiss(live.id);
      }}
      className="shrink-0 font-semibold text-accent hover:underline"
    >
      {live.actionLabel ?? "Undo"}
    </button>
  );

  if (variant === "inline") {
    return (
      <AnimatePresence>
        {live && (
          <motion.span
            key={live.id}
            role="status"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.16 }}
            className={`flex min-w-0 shrink items-center gap-2 text-[11.5px] ${live.tone === "error" ? "text-status-red/90" : "text-white/70"}`}
          >
            <span className="max-w-[300px] truncate" title={live.message}>
              {live.message}
            </span>
            {undo}
          </motion.span>
        )}
      </AnimatePresence>
    );
  }

  return (
    <AnimatePresence>
      {live && (
        <motion.div
          key={live.id}
          role="status"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: 6 }}
          transition={{ duration: 0.16 }}
          className={`pointer-events-auto absolute right-4 bottom-[58px] left-4 z-30 mx-auto flex w-fit max-w-[90%] items-center gap-3 rounded-xl px-3 py-1.5 text-[12px] shadow-lg ring-1 ${
            live.tone === "error" ? "surface-menu text-status-red ring-status-red/35" : "surface-menu text-white/85 ring-white/10"
          }`}
        >
          <span className="min-w-0 truncate">{live.message}</span>
          {undo}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
