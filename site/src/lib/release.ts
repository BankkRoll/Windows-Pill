/**
 * The latest Pill release on GitHub, for the download buttons.
 *
 * The installers are assets of a GitHub Release, so their file names change
 * with every version. The page asks the GitHub API once per browser session
 * and falls back to the Releases page whenever that doesn't work (offline,
 * rate-limited, or no release published yet).
 */

import { useEffect, useState } from "react";

/** The repository's page on GitHub. */
export const REPO_URL = "https://github.com/BankkRoll/Windows-Pill";

/** Where every release and its installers are listed; the fallback for every download link. */
export const RELEASES_URL = `${REPO_URL}/releases`;

const LATEST_API = "https://api.github.com/repos/BankkRoll/Windows-Pill/releases/latest";
const CACHE_KEY = "pill:latest-release:v1";

/** What the download buttons need from a release. */
export interface Release {
  /** The tag without its leading "v", e.g. `0.2.0`. */
  version: string;
  /** The release's page on GitHub. */
  url: string;
  /** The NSIS installer (`Pill_x.y.z_x64-setup.exe`), per user, no admin prompt. */
  setup: string | null;
  /** ISO date the release was published. */
  publishedAt: string | null;
}

/** Where the lookup stands: still asking, found a release, or fell back to the Releases page. */
export type ReleaseState = { kind: "loading" } | { kind: "ready"; release: Release } | { kind: "fallback" };

interface GithubAsset {
  name: string;
  browser_download_url: string;
}

interface GithubRelease {
  tag_name: string;
  html_url: string;
  published_at: string | null;
  assets: GithubAsset[];
}

/**
 * Picks the installers out of a GitHub release.
 *
 * @param data - The body of `GET /repos/{owner}/{repo}/releases/latest`.
 * @returns The release, or `null` when the body isn't shaped like one.
 * @example
 * parseRelease({ tag_name: "v0.2.0", html_url, published_at, assets }); // { version: "0.2.0", setup: ".../Pill_0.2.0_x64-setup.exe", ... }
 */
export function parseRelease(data: unknown): Release | null {
  if (!data || typeof data !== "object") return null;
  const r = data as Partial<GithubRelease>;
  if (typeof r.tag_name !== "string" || typeof r.html_url !== "string" || !Array.isArray(r.assets)) return null;
  const find = (test: (name: string) => boolean) =>
    r.assets!.find((a) => typeof a?.name === "string" && test(a.name.toLowerCase()) && isHttps(a.browser_download_url))
      ?.browser_download_url ?? null;
  return {
    version: r.tag_name.replace(/^v/, ""),
    url: isHttps(r.html_url) ? r.html_url : RELEASES_URL,
    setup: find((n) => n.endsWith("-setup.exe")),
    publishedAt: typeof r.published_at === "string" ? r.published_at : null,
  };
}

function isHttps(url: unknown): url is string {
  return typeof url === "string" && url.startsWith("https://");
}

// SECURITY: sessionStorage can hold anything another script on the origin
// wrote, so the cached value is parsed and validated like a fresh response.
function readCache(): Release | null {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const cached = JSON.parse(raw) as Partial<Release>;
    if (typeof cached.version !== "string" || !isHttps(cached.url)) return null;
    return {
      version: cached.version,
      url: cached.url,
      setup: isHttps(cached.setup) ? cached.setup : null,
      publishedAt: typeof cached.publishedAt === "string" ? cached.publishedAt : null,
    };
  } catch {
    return null;
  }
}

function writeCache(release: Release) {
  try {
    sessionStorage.setItem(CACHE_KEY, JSON.stringify(release));
  } catch {
    // Private windows and blocked storage only lose the cache.
  }
}

let pending: Promise<Release | null> | null = null;

/**
 * Fetches the latest release once per page load, shared by every download
 * button on the page.
 *
 * @returns The release, or `null` on any failure.
 */
export function fetchLatestRelease(): Promise<Release | null> {
  const cached = readCache();
  if (cached) return Promise.resolve(cached);
  pending ??= fetch(LATEST_API, { headers: { Accept: "application/vnd.github+json" } })
    .then((res) => (res.ok ? res.json() : null))
    .then((body: unknown) => {
      const release = parseRelease(body);
      if (release) writeCache(release);
      return release;
    })
    .catch(() => null);
  return pending;
}

/**
 * The latest release for a download button.
 *
 * @returns `loading` until the API answers, then `ready` or `fallback`.
 */
export function useLatestRelease(): ReleaseState {
  const [state, setState] = useState<ReleaseState>(() => {
    const cached = readCache();
    return cached ? { kind: "ready", release: cached } : { kind: "loading" };
  });
  useEffect(() => {
    if (state.kind !== "loading") return;
    let alive = true;
    void fetchLatestRelease().then((release) => {
      if (alive) setState(release ? { kind: "ready", release } : { kind: "fallback" });
    });
    return () => {
      alive = false;
    };
  }, [state.kind]);
  return state;
}
