/**
 * Download links that follow the latest GitHub release: the installer of the
 * newest version when the GitHub API answers, the Releases page otherwise.
 */

import { ArrowDownload16Regular } from "@fluentui/react-icons";
import { RELEASES_URL, useLatestRelease } from "../lib/release";

/** Props for {@link DownloadButton}. */
export interface DownloadButtonProps {
  /** `large` for the hero and the download section, `small` for the site header. */
  size?: "large" | "small";
  /** Show the version and size note under the button. */
  showMeta?: boolean;
}

/**
 * The primary download button.
 *
 * - While the release is being looked up it already links to the Releases
 *   page, so a click never goes nowhere.
 * - With a release it links straight to `Pill_<version>_x64-setup.exe` and
 *   names the version.
 * - Without one (offline, rate-limited, nothing published yet) it stays on
 *   the Releases page.
 */
export function DownloadButton({ size = "large", showMeta = false }: DownloadButtonProps) {
  const state = useLatestRelease();
  const release = state.kind === "ready" ? state.release : null;
  const href = release?.setup ?? release?.url ?? RELEASES_URL;

  const button = size === "large" ? "h-11 gap-2 rounded-xl px-5 text-[15px]" : "h-8 gap-1.5 rounded-lg px-3 text-[13px]";

  return (
    <div className={`flex flex-col ${size === "large" ? "items-start gap-2.5" : ""}`}>
      <a
        href={href}
        className={`inline-flex items-center font-semibold text-on-accent bg-accent transition-colors hover:bg-[#4a8cff] ${button}`}
      >
        <ArrowDownload16Regular className={size === "large" ? "size-[18px]" : "size-4"} aria-hidden />
        {size === "large" ? "Download for Windows" : "Download"}
      </a>
      {showMeta && (
        <p className="text-[13px] text-white/45" aria-live="polite">
          {state.kind === "loading" && "Looking up the latest version…"}
          {release && (
            <>
              Version {release.version} · 64-bit installer, no admin rights needed ·{" "}
              <a href={release.url} className="text-white/65 underline-offset-2 hover:text-white hover:underline">
                Release notes
              </a>
            </>
          )}
          {state.kind === "fallback" && (
            <>
              Installers are on{" "}
              <a href={RELEASES_URL} className="text-white/65 underline-offset-2 hover:text-white hover:underline">
                GitHub Releases
              </a>
              .
            </>
          )}
        </p>
      )}
    </div>
  );
}
