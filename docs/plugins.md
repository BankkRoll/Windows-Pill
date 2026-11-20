# Plugins

Plugins add signals and a live item to the collapsed notch, cards and alerts
to Today, and tabs to the panel's header. This page installs
[CVE watch](../plugins/cve-watch/), shows how it works and builds a tiny
plugin. Every rule and limit is in the
[plugin specification](../plugins/README.md).

| Built-in plugin | Shows | On by default |
| --- | --- | --- |
| Now playing | Artwork in the notch; a Today card with controls, seek bar, shuffle, repeat, the player's volume and a player switcher | Yes |
| Windows notifications | The Action Center count; a Today card to read, dismiss or clear them (needs Windows' notification permission) | No |

## Install CVE watch

With [Node.js](https://nodejs.org) 20 or later on PATH, link the plugin into
Pill's plugins folder from a clone of this repository (or copy it there):

```powershell
New-Item -ItemType Junction -Path "$env:APPDATA\Pill\plugins\cve-watch" -Target (Resolve-Path .\plugins\cve-watch)
```

In **Settings → Plugins**, press **Reload**, turn on **CVE watch** and confirm
with **Run it**. You get alerts with **Fix with an agent**, a Today card and a
**Security** tab.

## How CVE watch works

```json plugin.json
{
  "id": "cve-watch",
  "name": "CVE watch",
  "description": "Checks your projects' dependencies against OSV.dev and lists the advisories that affect them.",
  "command": "node",
  "args": ["plugin.mjs"],
  "capabilities": ["projects", "feed", "widget", "tab"],
  "tab": { "title": "Security", "icon": "shield" }
}
```

You approve the command line, `+projects`, `+tasks` and the folder; if any
changes, Pill asks again ([Consent](../plugins/README.md#consent)). Pill sends:

```jsonl
{"type":"hello","protocol":1,"pill":"0.1.0"}
{"type":"projects","projects":[{"id":"p_1a2b3c4d5e","name":"Shop","path":"C:\\code\\shop"}]}
{"type":"view","project":"p_1a2b3c4d5e"}
{"type":"action","id":"check-now"}
```

CVE watch reads each project's lockfiles, asks OSV.dev about the packages and
answers:

```jsonl
{"type":"feed","id":"GHSA-35jh-r3h4-6jhm.2bf7a6df8d02","severity":"high","icon":"shield","title":"Command Injection in lodash","body":"lodash versions prior to 4.17.21 are vulnerable to Command Injection via the template function.","detail":"lodash 4.17.20 → 4.17.21 · CVE-2021-23337","url":"https://osv.dev/vulnerability/GHSA-35jh-r3h4-6jhm","project":"p_1a2b3c4d5e","task":{"title":"Update lodash to 4.17.21 (CVE-2021-23337)","notes":"lodash 4.17.20 (npm) is affected by GHSA-35jh-r3h4-6jhm / CVE-2021-23337.\nFixed in: 4.17.21"}}
{"type":"widget","title":"CVE watch","rows":[{"icon":"shield","text":"1 critical · 2 high in 2 projects","tone":"red"},{"icon":"clock","text":"Checked 14:05"}],"actions":[{"id":"check-now","label":"Check now","icon":"shield"}]}
{"type":"page","project":"p_1a2b3c4d5e","page":{"blocks":[{"type":"header","title":"Shop","subtitle":"No known vulnerabilities","icon":"shield","tone":"green"}]}}
{"type":"nav","items":[{"project":null,"badge":{"text":"3 critical","tone":"red"}},{"project":"p_1a2b3c4d5e","badge":{"text":"2 critical","tone":"red"}}]}
{"type":"error","message":"Couldn't reach OSV.dev (ENOTFOUND). Showing the last results; trying again in 10 minutes."}
```

Worth copying: ids that change only when the situation does (Pill remembers
dismissals by id), rows that reuse alert ids as
[anchors](../plugins/README.md#list-rows), pages sent only on `view` and when
they change, at most 40 lines every 1.1 seconds, and atomic writes to
`PILL_PLUGIN_DATA`.

## Build a tiny plugin

A 25-minute focus timer. Create `%APPDATA%\Pill\plugins\pomodoro\plugin.json`:

```json plugin.json
{
  "id": "pomodoro",
  "name": "Pomodoro",
  "description": "A 25-minute focus timer in the notch.",
  "command": "node",
  "args": ["plugin.js"],
  "capabilities": ["signals", "live", "widget"]
}
```

Next to it, `plugin.js` writes one JSON object per line and reacts to `action`
lines:

```js
const readline = require("node:readline");
const send = (message) => process.stdout.write(JSON.stringify(message) + "\n");
let endsAt = null;

function render() {
  const left = endsAt === null ? 0 : endsAt - Date.now();
  if (endsAt !== null && left <= 0) {
    endsAt = null;
    send({ type: "signal", id: "done", icon: "timer", tone: "green", count: 1, label: "Focus session done" });
  }
  if (endsAt === null) {
    send({ type: "live", clear: true });
    send({ type: "widget", title: "Pomodoro", rows: [{ icon: "timer", text: "Ready" }], actions: [{ id: "start", label: "Start" }] });
    return;
  }
  const s = Math.ceil(left / 1000);
  const time = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  send({ type: "live", icon: "timer", text: time, tone: "accent" });
  send({ type: "widget", title: "Pomodoro", rows: [{ icon: "timer", text: `${time} left` }] });
}

readline.createInterface({ input: process.stdin }).on("line", (line) => {
  try {
    if (JSON.parse(line).id === "start") {
      endsAt = Date.now() + 25 * 60 * 1000;
      send({ type: "signal", id: "done", icon: "timer", count: 0 });
    }
  } catch {}
});
setInterval(render, 1000);
render();
```

Try it without Pill (the empty line absorbs PowerShell 5.1's byte order mark):

```powershell
'', '{"type":"action","id":"start"}' | node plugin.js
```

Then **Reload** in **Settings → Plugins** and turn on **Pomodoro**. Use
**Restart** after code changes; Settings shows any refused line and why.
