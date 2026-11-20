/**
 * The site's header and footer, shared by the landing page, the docs and
 * the 404 page.
 */

import { REPO_URL } from "../lib/release";
import { notchPath } from "../lib/shapes";
import { DownloadButton } from "./Download";

/** The site's root URL, including the GitHub Pages base path. */
export const HOME = import.meta.env.BASE_URL;

/** The docs page. */
export const DOCS = `${import.meta.env.BASE_URL}docs/`;

/** A file or folder in the repository on GitHub, e.g. `plugins/README.md`. */
export function repoLink(path: string): string {
  return `${REPO_URL}/blob/main/${path}`;
}

/**
 * Pill's mark: the notch outline, as the app draws it, in a small square.
 *
 * @param props.className - Size classes.
 */
export function Mark({ className = "size-6" }: { className?: string }) {
  const d = notchPath({ width: 14, height: 9, radius: 4.5, shoulder: 3, shoulderDepth: 3 }, 2);
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden>
      <rect width="24" height="24" rx="6" fill="#18181a" />
      <rect x="0.5" y="0.5" width="23" height="23" rx="5.5" fill="none" stroke="rgb(255 255 255 / 0.1)" />
      <rect x="2" y="6" width="20" height="1.25" fill="#fff" />
      <path d={d} transform="translate(0 6)" fill="#fff" />
    </svg>
  );
}

/** Props for {@link SiteHeader}. */
export interface SiteHeaderProps {
  current: "home" | "docs" | "other";
}

/** The sticky top bar: name, Docs, GitHub and a small download button. */
export function SiteHeader({ current }: SiteHeaderProps) {
  const link = "rounded-md px-2.5 py-1.5 text-[14px] transition-colors hover:text-white";
  return (
    <header className="sticky top-0 z-40 border-b border-white/[0.06] bg-page/80 backdrop-blur-xl">
      <div className="mx-auto flex h-14 max-w-6xl items-center gap-4 px-4 sm:px-6">
        <a href={HOME} className="flex items-center gap-2 rounded-md font-display text-[16px] font-semibold text-white/95">
          <Mark />
          Pill
        </a>
        <nav aria-label="Site" className="ml-auto flex items-center gap-1 sm:gap-2">
          <a
            href={DOCS}
            className={`${link} ${current === "docs" ? "text-white" : "text-white/60"}`}
            aria-current={current === "docs" ? "page" : undefined}
          >
            Docs
          </a>
          <a href={REPO_URL} className={`${link} text-white/60`}>
            GitHub
          </a>
          <span className="hidden sm:block">
            <DownloadButton size="small" />
          </span>
        </nav>
      </div>
    </header>
  );
}

/** The footer: licence, repository and docs links. */
export function SiteFooter() {
  const link = "text-white/55 hover:text-white";
  return (
    <footer className="border-t border-white/[0.06]">
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-10 text-[13px] text-white/40 sm:flex-row sm:items-center sm:px-6">
        <p className="flex items-center gap-2">
          <Mark className="size-5 shrink-0" />
          <span>
            Pill is free and open source under the{" "}
            <a href={repoLink("LICENSE")} className={link}>
              Apache 2.0 licence
            </a>
            .
          </span>
        </p>
        <nav aria-label="Footer" className="flex gap-5 sm:ml-auto">
          <a href={DOCS} className={link}>
            Docs
          </a>
          <a href={`${REPO_URL}/releases`} className={link}>
            Releases
          </a>
          <a href={`${REPO_URL}/issues`} className={link}>
            Issues
          </a>
          <a href={REPO_URL} className={link}>
            Source
          </a>
        </nav>
      </div>
    </footer>
  );
}
