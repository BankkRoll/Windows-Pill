/**
 * GitHub Pages serves 404.html for any path it has no file for.
 */

import { DOCS, HOME, SiteFooter, SiteHeader } from "./Chrome";

/** The not-found page, with the way back. */
export function NotFound() {
  return (
    <>
      <SiteHeader current="other" />
      <main className="mx-auto flex min-h-[60vh] max-w-6xl flex-col justify-center px-4 py-24 sm:px-6">
        <p className="text-[14px] font-medium text-white/45">404</p>
        <h1 className="mt-2 font-display text-[36px] font-semibold tracking-[-0.015em] text-white">There's no page here.</h1>
        <p className="mt-3 max-w-xl text-[17px] leading-relaxed text-white/60">
          The link may be old, or the page may have moved into the docs.
        </p>
        <p className="mt-8 flex gap-6 text-[15px]">
          <a href={HOME} className="font-medium text-accent hover:underline">
            Home
          </a>
          <a href={DOCS} className="text-white/70 hover:text-white">
            Docs
          </a>
        </p>
      </main>
      <SiteFooter />
    </>
  );
}
