import {
  Dismiss16Regular,
  Maximize16Regular,
  Pin16Filled,
  Pin16Regular,
  SquareMultiple16Regular,
  Subtract16Regular,
  type FluentIcon,
} from "@fluentui/react-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { attempt } from "../lib/actions";
import { api } from "../lib/native";
import { usePillStore } from "../store";

/**
 * Caption buttons for the frameless detached window: pin on top (a
 * picture-in-picture mode), minimize, maximize/restore and close.
 */
export function WindowControls() {
  const maximized = usePillStore((s) => s.filled);
  const setFilled = usePillStore((s) => s.setFilled);
  const [pinned, setPinned] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    void win.isAlwaysOnTop().then(setPinned);
    // Snap and header double-clicks maximize natively, which overhangs the
    // screen; the core swaps that for an exact fill of the work area.
    let timer = 0;
    const unlisten = win.onResized(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void api.normalizeMaximize().then(setFilled).catch(() => {}), 60);
    });
    return () => {
      window.clearTimeout(timer);
      void unlisten.then((f) => f());
    };
  }, [setFilled]);

  const win = getCurrentWindow();
  const togglePin = async () => {
    await attempt(() => win.setAlwaysOnTop(!pinned));
    setPinned(!pinned);
  };

  return (
    <div className="ml-1.5 flex h-full items-stretch">
      <Caption
        icon={pinned ? Pin16Filled : Pin16Regular}
        label={pinned ? "Unpin from top" : "Keep on top"}
        active={pinned}
        onClick={() => void togglePin()}
      />
      <Caption icon={Subtract16Regular} label="Minimize" onClick={() => void attempt(() => win.minimize())} />
      <Caption
        icon={maximized ? SquareMultiple16Regular : Maximize16Regular}
        label={maximized ? "Restore" : "Maximize"}
        onClick={() => void attempt(() => api.toggleFill().then(setFilled))}
      />
      <Caption icon={Dismiss16Regular} label="Close" danger onClick={() => void attempt(() => win.close())} />
    </div>
  );
}

function Caption({
  icon: Icon,
  label,
  onClick,
  active,
  danger,
}: {
  icon: FluentIcon;
  label: string;
  onClick: () => void;
  active?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={`grid w-11 place-items-center transition-colors ${
        danger ? "hover:bg-[#c42b1c] hover:text-on-accent" : "hover:bg-white/8 hover:text-white/90"
      } ${active ? "text-accent" : "text-white/55"}`}
    >
      <Icon className="size-4" />
    </button>
  );
}
