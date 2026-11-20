import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const openLink = vi.fn();
const writeText = vi.fn((_text: string) => Promise.resolve());

vi.mock("./actions", () => ({
  openLink: (url: string) => openLink(url),
  attempt: async <T>(op: () => Promise<T>) => {
    try {
      return await op();
    } catch {
      return undefined;
    }
  },
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: (text: string) => writeText(text) }));

const { handleMarkdownClick } = await import("./markdown");

/** Just enough of a DOM element for the handler: `closest` over a fixed ancestor chain. */
function fakeElement(chain: Record<string, object | null>) {
  return { closest: (selector: string) => chain[selector] ?? null };
}

function click(target: unknown) {
  const preventDefault = vi.fn();
  handleMarkdownClick({ target: target as EventTarget, preventDefault });
  return preventDefault;
}

describe("handleMarkdownClick", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", globalThis);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    openLink.mockClear();
    writeText.mockClear();
  });

  it("opens links outside the webview instead of navigating it", () => {
    const link = { getAttribute: (name: string) => (name === "href" ? "https://example.com/a" : null) };
    const prevented = click(fakeElement({ a: link }));
    expect(prevented).toHaveBeenCalledOnce();
    expect(openLink).toHaveBeenCalledWith("https://example.com/a");
  });

  it("still cancels navigation for a link without an href", () => {
    const prevented = click(fakeElement({ a: { getAttribute: () => null } }));
    expect(prevented).toHaveBeenCalledOnce();
    expect(openLink).not.toHaveBeenCalled();
  });

  it("copies the code block next to a Copy button", async () => {
    const copy = {
      textContent: "Copy",
      parentElement: { querySelector: (s: string) => (s === "pre" ? { textContent: "pnpm test" } : null) },
    };
    const prevented = click(fakeElement({ "button[data-copy]": copy }));
    expect(prevented).not.toHaveBeenCalled();
    expect(writeText).toHaveBeenCalledWith("pnpm test");
    await vi.waitFor(() => expect(copy.textContent).toBe("Copied"));
    vi.advanceTimersByTime(1200);
    expect(copy.textContent).toBe("Copy");
  });

  it("leaves the Copy label alone when the clipboard fails", async () => {
    writeText.mockImplementationOnce(() => Promise.reject(new Error("denied")));
    const copy = { textContent: "Copy", parentElement: { querySelector: () => ({ textContent: "x" }) } };
    click(fakeElement({ "button[data-copy]": copy }));
    await Promise.resolve();
    await Promise.resolve();
    expect(copy.textContent).toBe("Copy");
  });

  it("ignores clicks on plain text and non-element targets", () => {
    expect(click(fakeElement({}))).not.toHaveBeenCalled();
    expect(click(null)).not.toHaveBeenCalled();
    expect(click({})).not.toHaveBeenCalled();
    expect(openLink).not.toHaveBeenCalled();
  });
});
