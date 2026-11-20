/**
 * Asks once per project how agents' work should be checked when no test
 * command is set: use the one found in the project, have an agent set
 * tests up, or stop asking.
 */

import { Beaker16Regular } from "@fluentui/react-icons";
import { useEffect, useState } from "react";
import { attempt, startAgent } from "../lib/actions";
import { api, type FoundCommand, type Project } from "../lib/native";

/** Title of the task that sets a project's tests up. */
const SETUP_TITLE = "Set up automated tests";

/** Notes for the task that sets a project's tests up. */
const SETUP_NOTES = `Give this project automated tests that run with one command.

- Look for an existing test setup first and build on it.
- Otherwise add the standard test runner for the stack, with a \`test\` script or target where the stack expects it.
- Cover the core logic with real tests, not placeholders. Fix any bugs they find.
- Make sure the whole suite passes, and say in the PR description which command runs it.`;

/**
 * The prompt, or nothing when the project has a test command or the user
 * declined.
 *
 * @param project - Project to check.
 * @param ran - Command a run found and ran itself; offers to keep it.
 */
export function TestSetup({ project, ran }: { project: Project; ran?: string | null }) {
  const [found, setFound] = useState<FoundCommand | null | undefined>(ran ? { command: ran, source: "" } : undefined);
  const [busy, setBusy] = useState(false);
  // Once a setup task exists, its own run is the answer; don't offer another.
  const pending = project.tasks.some((t) => t.title === SETUP_TITLE && t.status !== "done");
  const needed = !project.settings.testCommand.trim() && !project.settings.skipTestSetup && (!!ran || !pending);

  useEffect(() => {
    if (!needed || ran) return;
    let live = true;
    void api
      .detectChecks(project.id)
      .then((d) => live && setFound(d.test))
      .catch(() => live && setFound(null));
    return () => {
      live = false;
    };
  }, [needed, ran, project.id]);

  if (!needed || found === undefined) return null;

  const save = (patch: Partial<Project["settings"]>) =>
    attempt(() => api.saveProjectSettings(project.id, { ...project.settings, ...patch }));

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    await fn();
    setBusy(false);
  };

  const setUp = async () => {
    const task = await attempt(() =>
      api.createTask(project.id, {
        title: SETUP_TITLE,
        notes: SETUP_NOTES,
      }),
    );
    if (task) await startAgent(project, task);
  };

  return (
    <div className="flex items-start gap-2.5 rounded-xl bg-white/[0.04] p-2.5 ring-1 ring-white/[0.06]">
      <Beaker16Regular className="mt-0.5 size-4 shrink-0 text-white/50" />
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <p className="text-[12px] leading-relaxed text-white/70">
          {ran ? (
            <>
              This run's tests ran with <code className="text-white/90">{ran}</code>. Keep it as {project.name}'s test command so every run
              is checked?
            </>
          ) : found ? (
            <>
              {project.name} has no test command, so agents' changes aren't checked. Found{" "}
              <code className="text-white/90">{found.command}</code> in {found.source}.
            </>
          ) : (
            <>
              {project.name} has no tests Pill can run, so agents' changes aren't checked. An agent can set them up: the right runner for
              the stack, real tests for the core logic, and one command to run them.
            </>
          )}
        </p>
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(found ? () => save({ testCommand: found.command }) : setUp)}
            className="inline-flex h-7 items-center rounded-lg bg-accent px-3 text-[12px] font-semibold text-on-accent hover:brightness-110 disabled:opacity-40"
          >
            {ran ? "Keep it" : found ? "Use it" : "Set up tests"}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(() => save({ skipTestSetup: true }))}
            title="Pill stops asking. You can set a test command any time in the project's settings."
            className="inline-flex h-7 items-center rounded-lg px-2.5 text-[12px] text-white/55 hover:bg-white/8 hover:text-white/85 disabled:opacity-40"
          >
            Not now
          </button>
        </div>
      </div>
    </div>
  );
}
