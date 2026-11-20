import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

/**
 * The public site: a landing page, the docs and a 404 page, each its own HTML
 * entry so GitHub Pages serves every URL as a real file and deep links need
 * no client-side routing.
 */
export default defineConfig({
  // NOTE: GitHub Pages serves a project site under the repository name.
  base: "/Windows-Pill/",
  plugins: [react(), tailwindcss()],
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        docs: resolve(import.meta.dirname, "docs/index.html"),
        notFound: resolve(import.meta.dirname, "404.html"),
      },
    },
  },
});
