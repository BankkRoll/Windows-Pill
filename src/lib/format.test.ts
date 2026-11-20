import { describe, expect, it } from "vitest";
import { hostOf, isWebUrl } from "./format";

describe("isWebUrl", () => {
  it("accepts http and https in any case", () => {
    expect(isWebUrl("https://example.com")).toBe(true);
    expect(isWebUrl("http://example.com/a?b=1")).toBe(true);
    expect(isWebUrl("HTTPS://Example.com")).toBe(true);
    expect(isWebUrl("Http://example.com")).toBe(true);
  });

  it("rejects other schemes, relative links and junk", () => {
    expect(isWebUrl("javascript:alert(1)")).toBe(false);
    expect(isWebUrl("file:///C:/Windows/System32")).toBe(false);
    expect(isWebUrl("tauri://localhost/index.html")).toBe(false);
    expect(isWebUrl("ms-settings:privacy-microphone")).toBe(false);
    expect(isWebUrl("/relative/path")).toBe(false);
    expect(isWebUrl("#heading")).toBe(false);
    expect(isWebUrl("")).toBe(false);
  });
});

describe("hostOf", () => {
  it("drops a leading www.", () => {
    expect(hostOf("https://www.github.com/a/b")).toBe("github.com");
    expect(hostOf("https://news.ycombinator.com/item?id=1")).toBe("news.ycombinator.com");
  });

  it("keeps a port", () => {
    expect(hostOf("http://localhost:1420/")).toBe("localhost:1420");
  });

  it("falls back for unparseable links", () => {
    expect(hostOf("not a url")).toBe("link");
  });
});
