import { Pin16Regular } from "@fluentui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useKeyboardFocus } from "../hooks/useKeyboardFocus";
import { attempt, reportError } from "../lib/actions";
import { handleMarkdownClick, renderMarkdown } from "../lib/markdown";
import { api, errorMessage, type Project } from "../lib/native";
import { usePillStore } from "../store";
import { Segmented } from "./controls";

type Mode = "split" | "edit" | "preview";
const SAVE_DELAY_MS = 600;

/**
 * Editor for a project's `.pill/notes.md`, shown from the Chat sidebar:
 * markdown with live preview, autosave, and pinning snippets to tasks as
 * agent context.
 *
 * @param props.project - Project whose notes to edit.
 */
export function NotesEditor({ project }: { project: Project }) {
  const [mode, setMode] = useState<Mode>("split");
  const [text, setText] = useState(project.notes);
  const [conflict, setConflict] = useState<string | null>(null);
  const [selection, setSelection] = useState("");
  const base = useRef(project.notes);
  const dirty = text !== base.current;
  const focus = useKeyboardFocus();
  const html = useMemo(() => renderMarkdown(text), [text]);

  // Take outside edits when there's nothing unsaved; otherwise the save
  // path detects the conflict.
  useEffect(() => {
    if (!dirty && project.notes !== base.current) {
      base.current = project.notes;
      setText(project.notes);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- react to disk changes only
  }, [project.notes]);

  const latest = useRef({ text, conflict });
  latest.current = { text, conflict };
  // Saves run one after another: each one's base is the text the previous
  // one wrote, so overlapping saves would read as a conflict with ourselves.
  const queue = useRef(Promise.resolve());
  const save = useCallback(
    (onError: (e: unknown) => void) => {
      queue.current = queue.current.then(async () => {
        const { text, conflict } = latest.current;
        if (conflict || text === base.current) return;
        try {
          await api.saveNotes(project.id, text, base.current);
          base.current = text;
        } catch (e) {
          onError(e);
        }
      });
    },
    [project.id],
  );

  useEffect(() => {
    if (!dirty || conflict) return;
    const id = window.setTimeout(() => save((e) => setConflict(errorMessage(e))), SAVE_DELAY_MS);
    return () => window.clearTimeout(id);
  }, [text, dirty, conflict, save]);

  // Leaving the notes (another chat, another tab, the notch closing) must
  // not drop the last few keystrokes still waiting for the autosave.
  useEffect(() => () => save(reportError), [save]);

  const resolve = async (keepMine: boolean) => {
    if (keepMine) {
      const ok = await attempt(() => api.saveNotes(project.id, text, project.notes).then(() => true));
      if (ok) base.current = text;
    } else {
      base.current = project.notes;
      setText(project.notes);
    }
    setConflict(null);
  };

  return (
    <div className="flex h-full flex-col gap-2 px-1 pb-1">
      <div className="flex items-center justify-between gap-2">
        <div className="w-48">
          <Segmented
            options={[
              { value: "edit", label: "Edit" },
              { value: "split", label: "Split" },
              { value: "preview", label: "Preview" },
            ]}
            value={mode}
            onChange={setMode}
          />
        </div>
        <div className="flex items-center gap-2">
          <AnimatePresence>{selection && <PinMenu project={project} snippet={selection} />}</AnimatePresence>
          <span className="text-[11px] text-white/35">{conflict ? "Not saved" : dirty ? "Saving…" : "Saved"}</span>
        </div>
      </div>

      {conflict && (
        <div className="flex items-center gap-2 rounded-lg bg-status-amber/12 px-3 py-1.5 text-[12px] text-status-amber">
          <span className="flex-1">notes.md changed on disk while you were editing.</span>
          <button type="button" className="font-semibold hover:underline" onClick={() => void resolve(false)}>
            Load theirs
          </button>
          <button type="button" className="font-semibold hover:underline" onClick={() => void resolve(true)}>
            Keep mine
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1 gap-2">
        {mode !== "preview" && (
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onSelect={(e) => {
              const t = e.currentTarget;
              setSelection(t.value.slice(t.selectionStart, t.selectionEnd).trim());
            }}
            {...focus.props}
            onKeyDown={(e) => {
              if (e.key === "Escape") focus.release(e.currentTarget);
            }}
            spellCheck={false}
            placeholder={"# Ideas\n\nContext, links and decisions for this project.\nAgents read these notes."}
            aria-label="Project notes"
            className="min-w-0 flex-1 resize-none rounded-xl bg-white/5 p-3 font-mono text-[12px] leading-relaxed text-white/85 outline-none placeholder:text-white/25 focus:bg-white/7"
          />
        )}
        {mode !== "edit" &&
          (text.trim() ? (
            <article
              onClick={handleMarkdownClick}
              className="md scroll-fade min-w-0 flex-1 overflow-y-auto rounded-xl bg-white/[0.025] p-3"
              dangerouslySetInnerHTML={{ __html: html }}
            />
          ) : (
            <div className="grid min-w-0 flex-1 place-items-center rounded-xl bg-white/[0.025] p-6 text-center text-[12px] leading-relaxed text-white/30">
              <p>
                Markdown renders here.
                <br />
                Select text in the editor to pin it to a task as agent context.
              </p>
            </div>
          ))}
      </div>
    </div>
  );
}

function PinMenu({ project, snippet }: { project: Project; snippet: string }) {
  const [open, setOpen] = useState(false);
  const open_tasks = project.tasks.filter((t) => t.status !== "done");
  const showToast = usePillStore((s) => s.showToast);

  const pin = async (taskId: string) => {
    const task = project.tasks.find((t) => t.id === taskId);
    if (!task) return;
    setOpen(false);
    const ok = await attempt(() => api.updateTask(project.id, taskId, { pinnedNotes: [...task.pinnedNotes, snippet] }).then(() => true));
    if (ok) showToast({ message: `Pinned to “${task.title}”`, tone: "info" });
  };

  return (
    <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} className="relative">
      <button
        type="button"
        // Keep the textarea selection alive while choosing a task.
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => setOpen((o) => !o)}
        disabled={open_tasks.length === 0}
        title={open_tasks.length ? "Pin the selected text to a task as agent context" : "No open tasks in this project"}
        className="inline-flex h-6 items-center gap-1 rounded-md bg-accent/18 px-2 text-[11px] font-medium text-accent disabled:opacity-40"
      >
        <Pin16Regular className="size-3.5" /> Pin to task
      </button>
      {open && (
        <ul className="absolute top-7 right-0 z-20 max-h-56 w-64 overflow-y-auto rounded-xl surface-menu p-1 shadow-xl ring-1 ring-white/10">
          {open_tasks.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void pin(t.id)}
                className="w-full truncate rounded-lg px-2 py-1.5 text-left text-[12px] text-white/85 hover:bg-white/8"
              >
                {t.title}
              </button>
            </li>
          ))}
        </ul>
      )}
    </motion.div>
  );
}
