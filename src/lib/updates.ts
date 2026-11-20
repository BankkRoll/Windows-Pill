/**
 * Updates from GitHub Releases.
 *
 * Responsibilities:
 * - Ask the updater plugin for a newer signed release (`latest.json`).
 * - Download it with progress, then install and relaunch on request.
 * - A quiet background check at most once a day in the notch window, which
 *   downloads a found update so the user only has to restart.
 * - Turn updater failures (offline, missing release, bad signature) into
 *   sentences.
 *
 * The plugin verifies every download against the public key in
 * `tauri.conf.json` before it will install it.
 */

import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { create } from "zustand";
import { errorMessage, isTauri } from "./native";

/** Where the update flow is. */
export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "unconfigured" }
  | { kind: "checking" }
  | { kind: "current"; checkedAt: number }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "downloading"; version: string; downloaded: number; total: number | null }
  | { kind: "ready"; version: string; notes: string | null }
  | { kind: "installing"; version: string }
  | { kind: "error"; message: string };

interface UpdateStore {
  status: UpdateStatus;
  set: (status: UpdateStatus) => void;
}

/** Update state for this window. */
export const useUpdateStore = create<UpdateStore>((set) => ({
  status: { kind: "idle" },
  set: (status) => set({ status }),
}));

/** Minimum time between quiet background checks. */
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

const LAST_CHECK_KEY = "pill.updates.lastCheck";

// The plugin's Update holds a native resource; it can't live in the store.
let pending: Update | null = null;

/**
 * Whether a quiet check is due.
 *
 * @param lastCheck - Epoch milliseconds of the last check, or `null` if never.
 * @param now - Current epoch milliseconds.
 * @returns True when a day has passed, or the stored time is unusable.
 * @example shouldCheck(null, Date.now()) // true
 */
export function shouldCheck(lastCheck: number | null, now: number): boolean {
  if (lastCheck === null || !Number.isFinite(lastCheck) || lastCheck > now) return true;
  return now - lastCheck >= CHECK_INTERVAL_MS;
}

/**
 * Download progress as a whole percentage.
 *
 * @param downloaded - Bytes received so far.
 * @param total - Size from the server, if it sent one.
 * @returns 0–100, or `null` when the size is unknown.
 */
export function progressPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((downloaded / total) * 100)));
}

/**
 * Turns an updater error into a sentence a person can act on.
 *
 * @param e - Rejection from the updater plugin.
 * @returns A short explanation.
 * @example describeUpdateError("error sending request for url") // "Couldn't reach GitHub. …"
 */
export function describeUpdateError(e: unknown): string {
  const raw = errorMessage(e);
  const text = raw.toLowerCase();
  if (/(error sending request|dns|network|connect|offline|timed out|timeout|unreachable|os error 1100)/.test(text)) {
    return "Couldn't reach GitHub. Check your connection and try again.";
  }
  if (text.includes("signature")) {
    return "The update failed its signature check and was not installed.";
  }
  if (/(valid release json|status code|404|not found)/.test(text)) {
    return "No release information was found for this app yet.";
  }
  return raw || "Something went wrong while updating.";
}

function readLastCheck(): number | null {
  try {
    const v = window.localStorage.getItem(LAST_CHECK_KEY);
    return v === null ? null : Number(v);
  } catch {
    return null;
  }
}

function writeLastCheck(at: number) {
  try {
    window.localStorage.setItem(LAST_CHECK_KEY, String(at));
  } catch {
    // Storage can be unavailable; the next launch simply checks again.
  }
}

let configured: Promise<boolean> | null = null;

function isConfigured(): Promise<boolean> {
  configured ??= invoke<boolean>("updates_configured").catch(() => false);
  return configured;
}

/**
 * Asks GitHub Releases for a newer version.
 *
 * @param options.quiet - Background check: failures leave the status alone
 *   and a found update is downloaded right away.
 */
export async function checkForUpdates({ quiet = false }: { quiet?: boolean } = {}): Promise<void> {
  const { status, set } = useUpdateStore.getState();
  if (!isTauri || ["checking", "downloading", "installing", "ready"].includes(status.kind)) return;
  if (!(await isConfigured())) {
    if (!quiet) set({ kind: "unconfigured" });
    return;
  }
  if (!quiet) set({ kind: "checking" });
  try {
    const update = await check({ timeout: 20_000 });
    writeLastCheck(Date.now());
    if (pending && pending !== update) void pending.close().catch(() => {});
    pending = update;
    if (!update) {
      set({ kind: "current", checkedAt: Date.now() });
      return;
    }
    set({ kind: "available", version: update.version, notes: update.body?.trim() || null });
    if (quiet) await downloadUpdate({ quiet: true });
  } catch (e) {
    if (quiet) console.warn("update check failed", e);
    else set({ kind: "error", message: describeUpdateError(e) });
  }
}

/**
 * Downloads the found update, reporting progress through the store.
 *
 * @param options.quiet - Failures go back to "available" instead of showing an error.
 */
export async function downloadUpdate({ quiet = false }: { quiet?: boolean } = {}): Promise<void> {
  const update = pending;
  const { set } = useUpdateStore.getState();
  if (!update) return;
  const notes = update.body?.trim() || null;
  let downloaded = 0;
  let total: number | null = null;
  set({ kind: "downloading", version: update.version, downloaded, total });
  try {
    await update.download((event: DownloadEvent) => {
      if (event.event === "Started") total = event.data.contentLength ?? null;
      else if (event.event === "Progress") downloaded += event.data.chunkLength;
      set({ kind: "downloading", version: update.version, downloaded, total });
    });
    set({ kind: "ready", version: update.version, notes });
  } catch (e) {
    if (quiet) {
      console.warn("update download failed", e);
      set({ kind: "available", version: update.version, notes });
    } else {
      set({ kind: "error", message: describeUpdateError(e) });
    }
  }
}

/**
 * Installs the downloaded update and restarts Pill.
 *
 * NOTE: On Windows the installer closes Pill itself and starts the new
 * version when it finishes, so `relaunch` only runs where it doesn't.
 */
export async function installUpdate(): Promise<void> {
  const update = pending;
  const { set } = useUpdateStore.getState();
  if (!update) return;
  set({ kind: "installing", version: update.version });
  try {
    await update.install();
    await relaunch();
  } catch (e) {
    set({ kind: "error", message: describeUpdateError(e) });
  }
}

/**
 * Starts the quiet daily check. Call once, from the notch window only, so
 * two windows never download the same update.
 *
 * @returns A function that stops the schedule.
 */
export function startBackgroundChecks(): () => void {
  if (!isTauri) return () => {};
  const tick = () => {
    if (shouldCheck(readLastCheck(), Date.now())) void checkForUpdates({ quiet: true });
  };
  // Give startup (scans, watchers, the first render) a head start.
  const first = window.setTimeout(tick, 30_000);
  // Pill tends to run for weeks; look again every few hours, still at most daily.
  const every = window.setInterval(tick, 3 * 60 * 60 * 1000);
  return () => {
    window.clearTimeout(first);
    window.clearInterval(every);
  };
}
