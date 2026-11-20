/**
 * Turns a run's flat activity log into the blocks of a chat-style
 * transcript: the agent's messages, runs of tool calls folded together,
 * Pill's own pipeline steps, and outcomes.
 */

import type { LogLine } from "./native";

/** One block of a run transcript, in order. */
export type TranscriptBlock =
  | { kind: "message"; text: string }
  /** Something the user said to the agent. */
  | { kind: "user"; text: string }
  /** Consecutive tool calls, as `Name detail` strings. */
  | { kind: "tools"; tools: string[] }
  | { kind: "step"; text: string }
  | { kind: "output"; text: string }
  | { kind: "result"; ok: boolean; text: string };

/**
 * Groups log lines into transcript blocks. Tool calls between two messages
 * collapse into one block, so a run reads as a conversation rather than a
 * stream of file names.
 *
 * @param lines - The run's activity, oldest first.
 * @returns Blocks to render, oldest first.
 * @example
 * transcriptBlocks(lines) // [{ kind: "tools", tools: ["Read a.ts", "Edit a.ts"] }, { kind: "message", text: "Done." }]
 */
export function transcriptBlocks(lines: readonly LogLine[]): TranscriptBlock[] {
  const blocks: TranscriptBlock[] = [];
  for (const line of lines) {
    const last = blocks[blocks.length - 1];
    switch (line.kind) {
      case "tool": {
        const call = `${line.name} ${line.detail}`.trim();
        if (last?.kind === "tools") last.tools.push(call);
        else blocks.push({ kind: "tools", tools: [call] });
        break;
      }
      case "text":
        blocks.push({ kind: "message", text: line.text });
        break;
      case "user":
        blocks.push({ kind: "user", text: line.text });
        break;
      case "step":
        blocks.push({ kind: "step", text: line.text });
        break;
      case "output":
        blocks.push({ kind: "output", text: line.text });
        break;
      case "result":
        blocks.push({ kind: "result", ok: line.ok, text: line.text });
        break;
    }
  }
  return blocks;
}

/**
 * Summarises tool calls like `Read src/a.ts` as "Read 3 · Grep 1".
 *
 * @param tools - Calls as `Name detail` strings.
 */
export function toolSummary(tools: readonly string[]): string {
  const counts = new Map<string, number>();
  for (const t of tools) {
    const name = t.split(" ")[0] ?? t;
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return [...counts.entries()].map(([n, c]) => (c > 1 ? `${n} ${c}` : n)).join(" · ");
}

/**
 * A run's saved log with the live lines this window already received
 * appended, each line once. Lines that arrived live before the history was
 * fetched are in the history too, so only live lines the history doesn't
 * have are kept. Identical lines are matched one for one, so a line that
 * really was logged twice stays twice.
 *
 * @param history - The run's log as stored by the core, oldest first.
 * @param live - Lines received as events, oldest first.
 * @returns The merged log, oldest first.
 */
export function mergeLog(history: readonly LogLine[], live: readonly LogLine[]): LogLine[] {
  const seen = new Map<string, number>();
  for (const line of history) {
    const key = JSON.stringify(line);
    seen.set(key, (seen.get(key) ?? 0) + 1);
  }
  const merged = [...history];
  for (const line of live) {
    const key = JSON.stringify(line);
    const left = seen.get(key) ?? 0;
    if (left > 0) seen.set(key, left - 1);
    else merged.push(line);
  }
  return merged;
}
