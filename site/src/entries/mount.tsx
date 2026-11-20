/**
 * Mounts one page of the site.
 */

import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import "../styles.css";

/**
 * Renders `page` into the document's `#root`.
 *
 * @param page - The page's root element.
 */
export function mount(page: ReactNode) {
  const root = document.getElementById("root");
  if (!root) throw new Error("The page has no #root element");
  createRoot(root).render(<StrictMode>{page}</StrictMode>);
}
