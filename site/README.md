# Pill site

The public site for Pill: a landing page and the docs, published to GitHub
Pages at `https://bankkroll.github.io/Windows-Pill/` by
`.github/workflows/pages.yml` on every push to `main` that touches `site/`.

It is its own small Vite, React, TypeScript and Tailwind project with its own
lockfile, separate from the app.

```sh
cd site
pnpm install
pnpm dev        # http://localhost:5173/Windows-Pill/
pnpm build      # type-check, then build to dist/
pnpm preview    # serve dist/ at http://localhost:4173/Windows-Pill/
pnpm format     # prettier at --print-width 140
```

## Layout

| Path | What lives there |
| --- | --- |
| `index.html`, `docs/index.html`, `404.html` | One HTML entry per page, so GitHub Pages serves every URL as a real file |
| `src/entries/` | The script each page loads |
| `src/site/` | Page content: landing, docs, header and footer, download button, requirements |
| `src/mock/` | The app's UI redrawn with demo data: notch, panel, Today, Agents and Security |
| `src/lib/shapes.ts` | The notch outline, copied from the app's `src/lib/shapes.ts` |
| `src/lib/release.ts` | Finds the latest release's installer through the GitHub API |
| `public/` | Favicon and the Open Graph image |

## Keeping it in step with the app

- The colours, fonts and surfaces at the top of `src/styles.css` are copied
  from the app's `src/index.css`.
- The mockups in `src/mock/` copy the markup and Tailwind classes of the real
  components named at the top of each file, without anything that calls
  Tauri. When a component's look changes, change its mockup too.
- The docs follow the repository's `README.md`, `docs/plugins.md`,
  `plugins/README.md` and `plugins/cve-watch/README.md`.

## Download button

The page asks `api.github.com/repos/BankkRoll/Windows-Pill/releases/latest`
once per browser session (cached in `sessionStorage`) and links to the
`*-setup.exe` asset. Until it answers, and whenever it fails, is rate-limited
or finds no release, the button links to the Releases page.

## Open Graph image

`public/og.png` (1200 × 630) is a screenshot of the hero. Regenerate it after
the hero changes much.
