import { attempt } from "../../lib/actions";
import { api, type SettingsPatch } from "../../lib/native";
import { usePillStore } from "../../store";
import { ModelPicker } from "../ModelPicker";
import { Segmented, Toggle } from "../controls";
import { Group, NumberField, PageHeader, Row, TimeField } from "./kit";

/** Run limits and autopilot. Every control saves as soon as it changes. */
export function AgentSettings() {
  const config = usePillStore((s) => s.config);
  const runs = usePillStore((s) => s.runs);
  const save = (patch: SettingsPatch) => void attempt(() => api.updateSettings(patch));
  const { agent, autopilot } = config;
  const active = runs.filter((r) => ["queued", "preparing", "running", "verifying", "shipping"].includes(r.state)).length;

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Agents"
        subtitle={active > 0 ? `${active} run${active === 1 ? "" : "s"} active or queued right now.` : "How agent runs are scheduled and bounded."}
      />

      <Group title="Default models" hint="Used by agents and chats unless a project or chat picks its own.">
        <Row label="Claude Code">
          <ModelPicker
            provider="claude"
            value={config.models.claude}
            inherited={{ model: "", effort: "", modelFrom: "cli", effortFrom: "cli" }}
            onChange={(claude) => save({ models: { ...config.models, claude } })}
          />
        </Row>
        <Row label="Codex">
          <ModelPicker
            provider="codex"
            value={config.models.codex}
            inherited={{ model: "", effort: "", modelFrom: "cli", effortFrom: "cli" }}
            onChange={(codex) => save({ models: { ...config.models, codex } })}
          />
        </Row>
      </Group>

      <Group title="Limits" hint="Apply to every run in every project.">
        <Row label="Runs at once" hint="Extra runs wait in the queue">
          <Segmented
            label="Runs at once"
            options={[1, 2, 3, 4].map((n) => ({ value: n, label: String(n), title: `${n} run${n === 1 ? "" : "s"} in parallel` }))}
            value={agent.maxParallelRuns}
            onChange={(maxParallelRuns) => save({ agent: { ...agent, maxParallelRuns } })}
          />
        </Row>
        <Row label="Max turns" hint="The run stops when the model reaches this many turns">
          <NumberField
            value={agent.maxTurns}
            min={1}
            max={500}
            suffix="turns"
            onCommit={(maxTurns) => save({ agent: { ...agent, maxTurns } })}
          />
        </Row>
        <Row label="Timeout" hint="Per phase: build, tests and shipping each get this long">
          <NumberField
            value={agent.timeoutMinutes}
            min={1}
            max={1440}
            suffix="min"
            onCommit={(timeoutMinutes) => save({ agent: { ...agent, timeoutMinutes } })}
          />
        </Row>
        <Row label="Fix failed checks" hint="Sends the failing test or lint output back to the agent before the run fails">
          <Segmented
            label="Fix failed checks"
            options={[0, 1, 2, 3].map((n) => ({
              value: n,
              label: n === 0 ? "Off" : String(n),
              title: n === 0 ? "Fail as soon as a check fails" : `Up to ${n} fix attempt${n === 1 ? "" : "s"}`,
            }))}
            value={agent.fixAttempts}
            onChange={(fixAttempts) => save({ agent: { ...agent, fixAttempts } })}
          />
        </Row>
      </Group>

      <Group title="Autopilot" hint="Starts hands-off tasks when a run slot is free.">
        <Row label="Start hands-off tasks automatically" hint="A task whose run failed or was stopped waits for you">
          <Toggle checked={autopilot.enabled} onChange={(enabled) => save({ autopilot: { ...autopilot, enabled } })} />
        </Row>
        {autopilot.enabled && (
          <Row
            label="Only between"
            hint={
              autopilot.windowStart && autopilot.windowEnd
                ? autopilot.windowStart > autopilot.windowEnd
                  ? "Overnight window, crossing midnight"
                  : "Daytime window"
                : "Empty means any time"
            }
          >
            <div className="flex items-center gap-1.5">
              <TimeField
                label="Window start"
                value={autopilot.windowStart}
                onCommit={(windowStart) => save({ autopilot: { ...autopilot, windowStart } })}
              />
              <span className="text-[11.5px] text-white/40">to</span>
              <TimeField
                label="Window end"
                value={autopilot.windowEnd}
                onCommit={(windowEnd) => save({ autopilot: { ...autopilot, windowEnd } })}
              />
              {(autopilot.windowStart || autopilot.windowEnd) && (
                <button
                  type="button"
                  onClick={() => save({ autopilot: { ...autopilot, windowStart: null, windowEnd: null } })}
                  className="h-7 rounded-lg px-2 text-[11.5px] text-white/50 hover:bg-white/8 hover:text-white/90"
                >
                  Any time
                </button>
              )}
            </div>
          </Row>
        )}
      </Group>

      <Group title="Safety">
        <Row label="Isolation" hint="Every run works in its own git worktree on an agent/ branch">
          <span className="text-[11.5px] text-white/45">Always on</span>
        </Row>
        <Row label="Shipping" hint="Only the agent branch is pushed, and PRs open as drafts">
          <span className="text-[11.5px] text-white/45">Always on</span>
        </Row>
        <Row label="API keys" hint="Removed from the agent's environment, so runs bill your subscription">
          <span className="text-[11.5px] text-white/45">Always on</span>
        </Row>
      </Group>
    </div>
  );
}
