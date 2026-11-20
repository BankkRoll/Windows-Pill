import { ArrowCounterclockwise16Regular, ArrowDownload16Regular, ArrowSync16Regular, FolderOpen16Regular } from "@fluentui/react-icons";
import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { useKeyboardFocus } from "../../hooks/useKeyboardFocus";
import { attempt, revealFile } from "../../lib/actions";
import { useWallpaper } from "../../lib/backdrop";
import { api, isTauri, type Appearance, type MonitorInfo, type SettingsPatch, type Theme } from "../../lib/native";
import { paletteAttributes, SURFACES, usePalette, type PaletteChoice } from "../../lib/theme";
import { checkForUpdates, downloadUpdate, installUpdate, progressPercent, useUpdateStore } from "../../lib/updates";
import { usePillStore } from "../../store";
import { Select } from "../controls";
import { Segmented, Toggle } from "../controls";
import { GlassTintLayer } from "../Glass";
import { Kbd } from "../ui";
import { GlassBackdropPicker } from "./GlassBackdropPicker";
import { Group, PageHeader, Row, SoftButton } from "./kit";

const ACCENTS = ["#2F7BFF", "#0A84FF", "#5E5CE6", "#BF5AF2", "#FF375F", "#FF9F0A", "#30D158", "#64D2FF"];

/** Hotkey, display, startup and app info. Every control saves immediately. */
export function GeneralSettings() {
  const config = usePillStore((s) => s.config);
  const save = (patch: SettingsPatch) => void attempt(() => api.updateSettings(patch));
  const palette = usePalette();

  return (
    <div className="flex flex-col gap-4">
      <PageHeader title="General" subtitle="How the notch looks, where it lives and how it responds." />

      <Group title="Appearance" hint="Applies to the notch and the pop-out window together.">
        <AppearancePicker value={config.appearance} onChange={(appearance) => save({ appearance })} />
        <ThemePicker value={config.theme} onChange={(theme) => save({ theme })} />
        {config.theme === "glass" && <GlassBackdropPicker />}
        {config.theme === "glass" && (
          <Row
            label="Glass tint"
            hint={
              palette === "light"
                ? "Sheer shows more wallpaper; frosted reads best on busy ones"
                : "Lighter shows more wallpaper; darker reads best on busy ones"
            }
          >
            <Segmented
              label="Glass tint"
              options={[
                { value: "light", label: palette === "light" ? "Sheer" : "Light" },
                { value: "medium", label: "Medium" },
                { value: "dark", label: palette === "light" ? "Frosted" : "Dark" },
              ]}
              value={config.glassTint}
              onChange={(glassTint) => save({ glassTint })}
            />
          </Row>
        )}
      </Group>

      <Group title="Keyboard">
        <Row label="Quick-add hotkey" hint="Opens the task input from any app">
          <HotkeyRecorder />
        </Row>
      </Group>

      <Group title="Display">
        <MonitorRow />
        <Row label="Accent color">
          <div className="flex items-center gap-1.5" role="radiogroup" aria-label="Accent color">
            {ACCENTS.map((c) => {
              const on = config.accent.toUpperCase() === c;
              return (
                <button
                  key={c}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  aria-label={c}
                  onClick={() => save({ accent: c })}
                  className={`size-4 rounded-full outline-offset-2 transition-transform hover:scale-110 ${on ? "outline-2 outline-white/80" : ""}`}
                  style={{ background: c }}
                />
              );
            })}
          </div>
        </Row>
      </Group>

      <Group title="Opening" hint="Delays stop the notch from opening when the cursor just passes by.">
        <Row
          label="Keep on top"
          hint={config.onTop ? "The notch stays above other windows" : "Windows can cover it; the hotkey still brings it up"}
        >
          <Toggle checked={config.onTop} onChange={(onTop) => save({ onTop })} />
        </Row>
        <Row
          label="Open the panel by"
          hint={config.openOn === "click" ? "Click the notch; moving away still closes it" : "Resting the cursor on the notch"}
        >
          <Segmented
            label="Open the panel by"
            options={[
              { value: "hover", label: "Hovering" },
              { value: "click", label: "Clicking" },
            ]}
            value={config.openOn}
            onChange={(openOn) => save({ openOn })}
          />
        </Row>
        <Row label="Open after hovering" hint={`${config.hoverOpenDelayMs} ms`}>
          <Segmented
            label="Open delay"
            options={[
              { value: 60, label: "Instant", title: "60 ms" },
              { value: 120, label: "Quick", title: "120 ms" },
              { value: 250, label: "Normal", title: "250 ms" },
              { value: 400, label: "Relaxed", title: "400 ms" },
            ]}
            value={config.hoverOpenDelayMs}
            onChange={(hoverOpenDelayMs) => save({ hoverOpenDelayMs })}
          />
        </Row>
        <Row label="Close after leaving" hint={`${config.hoverCloseDelayMs} ms`}>
          <Segmented
            label="Close delay"
            options={[
              { value: 200, label: "Quick", title: "200 ms" },
              { value: 350, label: "Normal", title: "350 ms" },
              { value: 600, label: "Relaxed", title: "600 ms" },
              { value: 1000, label: "Patient", title: "1 second" },
            ]}
            value={config.hoverCloseDelayMs}
            onChange={(hoverCloseDelayMs) => save({ hoverCloseDelayMs })}
          />
        </Row>
      </Group>

      <Group title="Startup">
        <Row label="Launch at sign-in" hint="Starts hidden in the tray and shows the notch">
          <Toggle checked={config.launchAtStartup} onChange={(launchAtStartup) => save({ launchAtStartup })} />
        </Row>
      </Group>

      <About />
    </div>
  );
}

type Desktop = CSSProperties;

/** The wallpaper behind the previews: the real blurred one on glass, otherwise a stand-in. */
function useDesktop(): Desktop {
  const wallpaper = useWallpaper();
  return wallpaper
    ? { backgroundImage: `url(${wallpaper.url})`, backgroundSize: "cover", backgroundPosition: "center top" }
    : { background: "linear-gradient(160deg, #5d6f94, #7d5f86 55%, #4f7e74)" };
}

/**
 * A notch over the desktop, painted by the real theme tokens for `choice`:
 * the preview carries its own data-* attributes, so it can show another
 * surface or palette than the page.
 */
function NotchPreview({ choice, desktop }: { choice: PaletteChoice; desktop: Desktop }) {
  return (
    <span
      {...paletteAttributes(choice)}
      className="absolute top-0 left-1/2 h-7 w-3/5 -translate-x-1/2 overflow-hidden rounded-b-[14px] shadow-[0_6px_14px] shadow-shade/40"
    >
      {choice.theme === "glass" ? (
        <>
          <span className="absolute inset-0 scale-150 blur-md" style={desktop} />
          <GlassTintLayer palette={choice.palette} />
          <span className="absolute inset-0 rounded-b-[14px] ring-1 ring-white/12 ring-inset" />
        </>
      ) : (
        <>
          <span className="absolute inset-0" style={{ background: "var(--notch-fill)" }} />
          {choice.palette === "light" && (
            <span className="absolute inset-0 rounded-b-[14px]" style={{ boxShadow: "inset 0 0 0 1px var(--notch-edge)" }} />
          )}
        </>
      )}
      <span className="absolute top-1/2 left-2.5 h-1 w-6 -translate-y-1/2 rounded-full bg-white/70" />
      <span className="absolute top-1/2 right-2.5 size-1.5 -translate-y-1/2 rounded-full bg-status-green" />
    </span>
  );
}

/** A selectable preview tile with a caption, shared by both pickers. */
function PreviewTile({
  on,
  onClick,
  label,
  hint,
  desktop,
  children,
}: {
  on: boolean;
  onClick: () => void;
  label: string;
  hint: string;
  desktop: Desktop;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={on}
      onClick={onClick}
      className={`flex flex-col gap-2 rounded-xl p-1.5 text-left transition-[background-color,box-shadow] ${
        on ? "bg-white/[0.07] ring-2 ring-accent" : "ring-1 ring-white/[0.07] hover:bg-white/[0.04]"
      }`}
    >
      <span className="relative block h-16 overflow-hidden rounded-lg" style={desktop}>
        {children}
      </span>
      <span className="px-0.5">
        <span className="block text-[12px] text-white/90">{label}</span>
        <span className="block truncate text-[10.5px] text-white/40">{hint}</span>
      </span>
    </button>
  );
}

const MODES: readonly { id: Appearance; label: string; hint: string }[] = [
  { id: "dark", label: "Dark", hint: "Light text on dark" },
  { id: "light", label: "Light", hint: "Dark text on light" },
  { id: "system", label: "Match Windows", hint: "" },
];

/**
 * Dark, light or following Windows, previewed on the current surface style.
 * Match Windows shows both halves.
 */
function AppearancePicker({ value, onChange }: { value: Appearance; onChange: (a: Appearance) => void }) {
  const theme = usePillStore((s) => s.config.theme);
  const glassTint = usePillStore((s) => s.config.glassTint);
  const systemLight = usePillStore((s) => s.systemLight);
  const desktop = useDesktop();
  return (
    <div className="grid grid-cols-3 gap-2 p-2.5" role="radiogroup" aria-label="Mode">
      {MODES.map((m) => (
        <PreviewTile
          key={m.id}
          on={value === m.id}
          onClick={() => onChange(m.id)}
          label={m.label}
          hint={m.id === "system" ? `Windows is in ${systemLight ? "light" : "dark"} mode` : m.hint}
          desktop={desktop}
        >
          {m.id === "system" ? (
            <>
              <span className="absolute inset-0 [clip-path:inset(0_50%_0_0)]">
                <NotchPreview choice={{ theme, palette: "dark", glassTint }} desktop={desktop} />
              </span>
              <span className="absolute inset-0 [clip-path:inset(0_0_0_50%)]">
                <NotchPreview choice={{ theme, palette: "light", glassTint }} desktop={desktop} />
              </span>
            </>
          ) : (
            <NotchPreview choice={{ theme, palette: m.id, glassTint }} desktop={desktop} />
          )}
        </PreviewTile>
      ))}
    </div>
  );
}

const THEMES: readonly Theme[] = ["glass", "graphite", "black"];

/**
 * Surface style tiles previewing the real surfaces in the current palette:
 * the actual blurred wallpaper behind a notch painted the way each style
 * paints it.
 */
function ThemePicker({ value, onChange }: { value: Theme; onChange: (t: Theme) => void }) {
  const glassTint = usePillStore((s) => s.config.glassTint);
  const palette = usePalette();
  const desktop = useDesktop();
  return (
    <div className="grid grid-cols-3 gap-2 p-2.5" role="radiogroup" aria-label="Surface">
      {THEMES.map((id) => (
        <PreviewTile key={id} on={value === id} onClick={() => onChange(id)} {...SURFACES[id][palette]} desktop={desktop}>
          <NotchPreview choice={{ theme: id, palette, glassTint }} desktop={desktop} />
        </PreviewTile>
      ))}
    </div>
  );
}

function About() {
  const configPath = usePillStore((s) => s.configPath);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (isTauri) void getVersion().then(setVersion);
  }, []);

  return (
    <Group title="About">
      <Row label="Pill" hint={version ? `Version ${version}` : undefined}>
        <span className="text-[11.5px] text-white/40">Apache-2.0</span>
      </Row>
      {isTauri && <UpdateRow />}
      {configPath && (
        <Row label="Config file" hint={<span className="font-mono text-[10.5px] select-text">{configPath}</span>}>
          <SoftButton icon={<FolderOpen16Regular className="size-4" />} onClick={() => revealFile(configPath)}>
            Show
          </SoftButton>
        </Row>
      )}
    </Group>
  );
}

const time = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

/** Check, download and install updates, with progress and the restart step. */
function UpdateRow() {
  const status = useUpdateStore((s) => s.status);
  const busy = status.kind === "checking" || status.kind === "installing";
  const check = () => void checkForUpdates();

  let hint: ReactNode;
  let action: ReactNode = null;
  switch (status.kind) {
    case "idle":
      hint = "Pill looks for new versions on GitHub once a day";
      action = (
        <SoftButton icon={<ArrowSync16Regular className="size-4" />} onClick={check}>
          Check for updates
        </SoftButton>
      );
      break;
    case "unconfigured":
      hint = "This build has no update address. Install a release to get updates.";
      break;
    case "checking":
      hint = "Checking GitHub…";
      action = (
        <SoftButton icon={<ArrowSync16Regular className="size-4 animate-spin" />} onClick={check} disabled>
          Checking…
        </SoftButton>
      );
      break;
    case "current":
      hint = `You're up to date · checked at ${time.format(status.checkedAt)}`;
      action = (
        <SoftButton icon={<ArrowSync16Regular className="size-4" />} onClick={check}>
          Check again
        </SoftButton>
      );
      break;
    case "available":
      hint = `Version ${status.version} is available`;
      action = (
        <SoftButton icon={<ArrowDownload16Regular className="size-4" />} onClick={() => void downloadUpdate()}>
          Download
        </SoftButton>
      );
      break;
    case "downloading": {
      const pct = progressPercent(status.downloaded, status.total);
      hint = (
        <span className="flex flex-col gap-1">
          <span>
            Downloading {status.version}
            {pct === null ? "…" : ` · ${pct}%`}
          </span>
          <span
            className="block h-1 w-40 overflow-hidden rounded-full bg-white/10"
            role="progressbar"
            aria-valuenow={pct ?? undefined}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <span
              className={`block h-full rounded-full bg-accent transition-[width] ${pct === null ? "w-1/3 animate-pulse" : ""}`}
              style={pct === null ? undefined : { width: `${pct}%` }}
            />
          </span>
        </span>
      );
      break;
    }
    case "ready":
      hint = `Version ${status.version} is ready. Pill restarts to finish installing.`;
      action = (
        <button
          type="button"
          onClick={() => void installUpdate()}
          className="inline-flex h-7 items-center gap-1.5 rounded-lg bg-accent px-2.5 text-[12px] font-medium text-on-accent hover:brightness-110"
        >
          <ArrowCounterclockwise16Regular className="size-4" />
          Restart to update
        </button>
      );
      break;
    case "installing":
      hint = `Installing ${status.version}; Pill will restart by itself`;
      action = (
        <SoftButton onClick={check} disabled={busy}>
          Installing…
        </SoftButton>
      );
      break;
    case "error":
      hint = <span className="text-status-red/90">{status.message}</span>;
      action = (
        <SoftButton icon={<ArrowSync16Regular className="size-4" />} onClick={check}>
          Try again
        </SoftButton>
      );
      break;
  }
  const notes = status.kind === "available" || status.kind === "ready" ? status.notes : null;

  return (
    <>
      <Row label="Updates" hint={hint}>
        {action}
      </Row>
      {notes && <p className="line-clamp-4 px-3 py-2 text-[11.5px] leading-snug whitespace-pre-line text-white/50 select-text">{notes}</p>}
    </>
  );
}

function MonitorRow() {
  const monitor = usePillStore((s) => s.config.monitor);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    void api.listMonitors().then(setMonitors);
  }, []);
  const only = monitors.length < 2;
  return (
    <Row label="Show on" hint={only ? "Only one display is connected" : "Falls back to primary if unplugged"}>
      <Select<number | null>
        label="Show on"
        value={monitor}
        disabled={only}
        align="end"
        options={[
          { value: null, label: "Primary display" },
          ...monitors.map((m) => ({
            value: m.index,
            label: m.name,
            detail: `${Math.round(m.width / m.scale)}×${Math.round(m.height / m.scale)}${m.primary ? " (primary)" : ""}`,
          })),
        ]}
        onChange={(v) => void attempt(() => api.updateSettings({ monitor: v }))}
        className="max-w-56"
      />
    </Row>
  );
}

const KEY_NAMES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/**
 * Converts a keydown into an accelerator string like `Ctrl+Shift+Space`.
 *
 * @param e - Keyboard event.
 * @returns The accelerator, or `null` for lone modifiers and combos without one.
 * @example acceleratorFrom({ code: "Space", ctrlKey: true, altKey: false, shiftKey: true, metaKey: false }) // "Ctrl+Shift+Space"
 */
export function acceleratorFrom(e: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string | null {
  let key: string | undefined;
  if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
  else if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
  else if (/^F\d{1,2}$/.test(e.code)) key = e.code;
  else key = KEY_NAMES[e.code];
  if (!key) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  // A global hotkey without Ctrl/Alt/Super would swallow normal typing.
  if (!e.ctrlKey && !e.altKey && !e.metaKey && !/^F\d/.test(key)) return null;
  return [...mods, key].join("+");
}

function HotkeyRecorder() {
  const hotkey = usePillStore((s) => s.config.hotkey);
  const hotkeyError = usePillStore((s) => s.hotkeyError);
  const setHotkeyError = usePillStore((s) => s.setHotkeyError);
  const [recording, setRecording] = useState(false);
  const focus = useKeyboardFocus();

  const onKeyDown = async (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return;
    e.preventDefault();
    if (e.key === "Escape") {
      setRecording(false);
      focus.release(e.currentTarget);
      return;
    }
    const accel = acceleratorFrom(e);
    if (!accel) return;
    setRecording(false);
    focus.release(e.currentTarget);
    try {
      await api.setHotkey(accel);
      setHotkeyError(null);
    } catch (err) {
      setHotkeyError(String(err));
    }
  };

  return (
    <div className="flex flex-col items-end gap-1">
      <button
        type="button"
        {...focus.props}
        onBlur={() => {
          focus.props.onBlur();
          setRecording(false);
        }}
        onClick={() => setRecording(true)}
        onKeyDown={(e) => void onKeyDown(e)}
        title={recording ? "Esc to cancel" : "Click to change"}
        className={`inline-flex h-7 min-w-36 items-center justify-center gap-1 rounded-lg px-2.5 text-[12px] outline-none ${
          recording ? "bg-accent/20 text-accent ring-1 ring-accent/60" : "bg-white/7 text-white/80 hover:bg-white/12"
        }`}
      >
        {recording ? "Press a shortcut…" : hotkey.split("+").map((k) => <Kbd key={k}>{k}</Kbd>)}
      </button>
      {hotkeyError && <p className="max-w-64 text-right text-[11px] text-status-red">{hotkeyError}</p>}
    </div>
  );
}
