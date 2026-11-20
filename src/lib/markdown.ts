/**
 * Markdown to safe HTML for notes, chat replies, run transcripts and issue
 * bodies, plus the click handling every rendered block shares.
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import DOMPurify from "dompurify";
import { marked } from "marked";
import { attempt, openLink } from "./actions";
import { isWebUrl } from "./format";

marked.setOptions({ gfm: true, breaks: true });

/**
 * Renders markdown to sanitized HTML.
 *
 * SECURITY: notes.md comes from repositories, which may be cloned from
 * anywhere. The webview can call into the Rust core, so raw HTML from a
 * note must never execute: everything goes through DOMPurify, and links are
 * opened by the app (never navigated in the webview).
 *
 * @param source - Markdown text.
 * @returns HTML safe to assign to `innerHTML`.
 */
export function renderMarkdown(source: string): string {
  const html = marked.parse(source, { async: false });
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true }, FORBID_TAGS: ["style", "form", "input"] });
}

/**
 * Renders a chat reply: {@link renderMarkdown} plus a copy button on every
 * code block. The button is handled by the chat's click delegation.
 *
 * @param source - Markdown text.
 * @returns Sanitized HTML.
 */
export function renderChatMarkdown(source: string): string {
  // Added after sanitizing, so the markup is ours and contains no user input.
  return renderMarkdown(source)
    .replace(/<pre>/g, '<div class="codeblock"><button type="button" class="copy" data-copy>Copy</button><pre>')
    .replace(/<\/pre>/g, "</pre></div>");
}

/**
 * Renders an issue body: {@link renderChatMarkdown}, with images turned into
 * links.
 *
 * SECURITY: Issue bodies are written by anyone who can open an issue.
 * Besides the sanitizing above, remote images are never loaded: an image
 * fetch would tell its author when and from where the issue was read (and
 * the app's CSP blocks them anyway, leaving broken icons).
 *
 * @param source - Issue markdown.
 * @returns Sanitized HTML.
 */
export function renderIssueMarkdown(source: string): string {
  const doc = new DOMParser().parseFromString(renderChatMarkdown(source), "text/html");
  for (const img of doc.querySelectorAll("img")) {
    const link = doc.createElement("a");
    const src = img.getAttribute("src") ?? "";
    link.textContent = img.getAttribute("alt")?.trim() || "Image";
    if (isWebUrl(src)) link.setAttribute("href", src);
    img.replaceWith(link);
  }
  return doc.body.innerHTML;
}

/** How long a code block's Copy button reads "Copied". */
const COPIED_MS = 1200;

/**
 * Click handler for any element holding rendered markdown HTML: links open
 * in the default browser, and the Copy buttons {@link renderChatMarkdown}
 * adds copy their code block.
 *
 * SECURITY: A plain click on a link would navigate the webview itself away
 * from the app, so every link click is cancelled and handed to `openLink`,
 * which only opens http(s) URLs, outside Pill.
 *
 * @param e - Click on (or inside) the markdown container.
 * @example <article onClick={handleMarkdownClick} dangerouslySetInnerHTML={{ __html: html }} />
 */
export function handleMarkdownClick(e: { target: EventTarget | null; preventDefault(): void }) {
  const target = e.target as Element | null;
  if (typeof target?.closest !== "function") return;
  const copy = target.closest<HTMLElement>("button[data-copy]");
  if (copy) {
    const code = copy.parentElement?.querySelector("pre")?.textContent ?? "";
    void attempt(() => writeText(code).then(() => true)).then((ok) => {
      if (!ok) return;
      copy.textContent = "Copied";
      window.setTimeout(() => (copy.textContent = "Copy"), COPIED_MS);
    });
    return;
  }
  const link = target.closest("a");
  if (!link) return;
  e.preventDefault();
  const href = link.getAttribute("href");
  if (href) openLink(href);
}
