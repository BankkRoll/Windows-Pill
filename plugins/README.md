# Pill plugin specification

Protocol version `1`; for a guided tour, see [docs/plugins.md](../docs/plugins.md).
An external plugin is a program that reads JSON lines on stdin and writes JSON
lines on stdout. Pill draws what it sends with its own components; plugins
never ship HTML, scripts or images. External plugins run on Windows only. The
built-in plugins reserve the ids `now-playing` and `notifications`.

## Folder layout

Pill reads `%APPDATA%\Pill\plugins\` at startup and on **Reload**. Every direct
subfolder with a `plugin.json` is a plugin, in name order, first 32 only; the
folder name needn't match the `id`, and when two share an `id` the first wins.
A folder whose manifest fails shows in Settings with the reason. Pill keeps
each plugin's data in `%APPDATA%\Pill\plugin-data\<id>\`, dismissals in
`feed-dismissed.json` and approvals in `config.json`.

## Manifest

```json plugin.json
{
  "id": "hello",
  "name": "Hello",
  "description": "Counts how often you wave.",
  "command": "node",
  "args": ["plugin.mjs"],
  "capabilities": ["live", "widget"]
}
```

UTF-8 JSON (a byte order mark is accepted), at most 64 KB. Unknown fields are
ignored; validation stops at the first problem. Bad JSON, a wrong type, or an
unknown capability or icon gives `plugin.json isn't valid: …`.

| Field | Required | Rules | Error |
| --- | --- | --- | --- |
| `id` | Yes | A token; unique; not a built-in id. | `id "…" must be 1-32 characters of a-z, 0-9, - or _`, `id "…" belongs to a built-in plugin`, `another plugin already uses the id "…"` |
| `name` | Yes | 1–40 characters after trimming, one line. | `name must be 1-40 characters on one line` |
| `description` | No | Up to 160 characters, one line. | `description must be at most 160 characters on one line` |
| `command` | Yes | 1–260 bytes after trimming, no control characters. | `command must be 1-260 characters` |
| `args` | No | Up to 32 strings, each under 1024 bytes, no NUL. | `at most 32 args`, `each arg must be under 1024 bytes` |
| `capabilities` | No | Default `["signals", "live", "widget"]`; `[]` allows only `error`. | |
| `tab` | With `tab` | `{"title", "icon"}`: title 1–16 characters, one line. | `"tab" needs the tab capability`, `the tab capability needs "tab": { "title": …, "icon": … }`, `tab title must be 1-16 characters on one line` |

`command` is a bare name (`node`: `<name>.exe`, `.cmd` or `.bat` on the current
PATH, then `%USERPROFILE%\.local\bin` and `%APPDATA%\npm`), a path relative to
the plugin folder that must stay inside it ("… is outside the plugin folder"),
or an absolute path that must exist. `.cmd` and `.bat` run through `cmd.exe`,
which refuses arguments with line breaks.

## Capabilities

| Capability | Lets the plugin | Approval line |
| --- | --- | --- |
| `signals` | Send `signal`: icons with a count in the collapsed notch. | |
| `live` | Send `live`: a few words or an icon in the collapsed notch. | |
| `widget` | Send `widget`: a card on Today. | |
| `feed` | Send `feed`: items in Today's Alerts. | `+tasks` |
| `tab` | Send `page` and `nav`, receive `view`: a header tab. Needs the manifest's `tab`. | `+tasks` |
| `projects` | Receive `projects`. Required for project pages, sidebar badges and feed items with a `project`. | `+projects` |

Other messages are refused ("signals isn't in this plugin's capabilities").

## Consent

A new plugin is off until you confirm its approval string:

```text
node plugin.mjs
+projects
+tasks
@C:\Users\you\AppData\Roaming\Pill\plugins\cve-watch
```

That is the command line (quoted the way Windows parses it), the approval lines
above, and the plugin folder with junctions and links resolved, stored under
`plugins.external.<id>` in `config.json`. Any difference at a start or
**Reload** stops the plugin ("The command, its folder or what it can access
changed since you turned this on") until approved again; `name`, `description`
and the `tab` title and icon don't count. A manifest that changes between
prompt and click refuses the click ("This plugin's command, folder or access
just changed"). Turning a plugin off forgets its approval.

## Process

Pill starts an approved plugin when Pill starts, when it's turned on, and on
**Restart**: it creates the data folder, starts `command` suspended with no
console window and the resolved plugin folder as working directory, puts it in
a Job Object, then lets it run. If containment fails, nothing runs: "Couldn't
start <command>: Couldn't contain the plugin process: …". Pill then writes
`hello`, `projects` (with `projects`) and a `view` per page already on screen
(with `tab`).

The environment is cleared, then holds only these of Pill's variables:
`SystemRoot`, `windir`, `SystemDrive`, `PATHEXT`, `TEMP`, `TMP`, `USERPROFILE`,
`HOMEDRIVE`, `HOMEPATH`, `APPDATA`, `LOCALAPPDATA`, `ComSpec`, `ProgramData`,
`ProgramFiles`, `ProgramFiles(x86)`, `ProgramW6432`, `CommonProgramFiles`,
`CommonProgramFiles(x86)`, `CommonProgramW6432`, `NUMBER_OF_PROCESSORS`,
`PROCESSOR_ARCHITECTURE`, `OS`, `USERNAME`, `COMPUTERNAME`, plus `PATH` (Pill's
PATH with user and machine entries added since it started), `PILL_PLUGIN_ID`,
`PILL_PROTOCOL` (`1`) and `PILL_PLUGIN_DATA` (the data folder, never deleted).

stdin carries Pill's messages; past 64 unread ones, more are dropped and
buttons report "The plugin isn't reading its input". stdout carries only the
plugin's messages. stderr is free-form; its last 2 KB explain an exit.

Turning the plugin off, removing its folder and pressing **Reload**, a changed
approval, **Restart** and quitting Pill close the Job Object: every process in
it ends at once, unwarned, so write files atomically. Everything the plugin
showed disappears. A plugin that exits shows `Stopped with exit code 1: <last
stderr line>` and **Restart**; Pill never restarts it by itself. **Reload**
keeps plugins whose folder (resolved too) and manifest are unchanged, and
restarts changed ones whose approval still matches.

## Transport

- UTF-8, one JSON object per line, `\n` or `\r\n`. Invalid UTF-8 is replaced;
  blank lines are ignored. Every message has a `type`; unknown fields are
  ignored, unknown types refused.
- A line may be at most **8 KB** (8,192 bytes, newline excluded); `page` and
  `nav` lines up to **128 KB** (131,072 bytes). Longer lines are skipped
  unbuffered; others over 8 KB are refused by `type` without being parsed.
- Pill takes at most 50 lines per second per plugin and drops the rest of that
  second, reporting it when the second ends. A refused line never stops the
  plugin; the latest reason shows in Settings.

**Text** loses control characters and invisible format characters (bidi
controls, U+061C, zero-width spaces, word joiners, byte order mark, soft
hyphen, tags; not the zero-width joiner), collapses whitespace and is cut to
end in `…`. Limits count characters, except ids and links (bytes). Task
`notes`, row `body` and `text` blocks keep line breaks. A required field left
empty is refused. Text is never markup.

| Id kind | Rule | Used by |
| --- | --- | --- |
| Token | 1–32 characters of `a-z`, `0-9`, `-`, `_` | manifest `id`, signal and button ids |
| Item id | 1–128 bytes of `A-Z`, `a-z`, `0-9` and `.` `_` `-` `:` `@` `/` `+` | feed item and list row ids |
| Project id | 1–96 characters of ASCII letters, digits, `_`, `-`, as sent in `projects` | `project`, `open` |

**Links.** A `url` is a plain `https://` URL of at most 2,048 bytes of visible
ASCII with a host and no user information. It opens in the browser on click.

## Messages from Pill

```jsonl
{"type":"hello","protocol":1,"pill":"0.1.0"}
{"type":"projects","projects":[{"id":"p_1a2b3c4d5e","name":"Shop","path":"C:\\code\\shop"}]}
{"type":"view","project":"p_1a2b3c4d5e"}
{"type":"view","project":"p_1a2b3c4d5e","closed":true}
{"type":"view","project":null}
{"type":"action","id":"check-now"}
{"type":"action","id":"snooze","item":"GHSA-35jh-r3h4-6jhm.2bf7a6df8d02"}
{"type":"action","id":"check-now","page":{"project":null}}
{"type":"action","id":"ignore-package","item":"npm/lodash","page":{"project":"p_1a2b3c4d5e"}}
```

| Message | Sent | Fields |
| --- | --- | --- |
| `hello` | First. | `protocol`, `pill` (Pill's version). |
| `projects` | With `projects`: after `hello` and when a project is added, removed, renamed or moved. | `projects[]`: `id`, `name`, `path`, sorted by name. |
| `view` | With `tab`: a page came on screen or left it. | `project` (`null`: overview), `closed`. |
| `action` | A click on a button the plugin currently shows. | `id`; `item` (feed item or row); `page` (`{"project"}`). |

## Messages to Pill

```jsonl
{"type":"signal","id":"builds","icon":"warning","tone":"red","count":2,"label":"2 builds failing"}
{"type":"signal","id":"builds","icon":"warning","count":0}
{"type":"live","icon":"timer","text":"18:42","tone":"accent"}
{"type":"live","clear":true}
{"type":"widget","title":"CVE watch","rows":[{"icon":"shield","text":"1 critical · 2 high in 2 projects","tone":"red"},{"icon":"clock","text":"Checked 14:05"}],"actions":[{"id":"check-now","label":"Check now","icon":"shield"}]}
{"type":"widget","clear":true}
{"type":"feed","id":"GHSA-35jh-r3h4-6jhm.2bf7a6df8d02","severity":"high","icon":"shield","title":"Command Injection in lodash","body":"lodash versions prior to 4.17.21 are vulnerable to Command Injection via the template function.","detail":"lodash 4.17.20 → 4.17.21 · CVE-2021-23337","url":"https://osv.dev/vulnerability/GHSA-35jh-r3h4-6jhm","project":"p_1a2b3c4d5e","task":{"title":"Update lodash to 4.17.21 (CVE-2021-23337)","notes":"lodash 4.17.20 (npm) is affected by GHSA-35jh-r3h4-6jhm / CVE-2021-23337.\nFixed in: 4.17.21"}}
{"type":"feed","id":"GHSA-35jh-r3h4-6jhm.2bf7a6df8d02","remove":true}
{"type":"feed","clear":true}
{"type":"error","message":"Couldn't reach OSV.dev (ENOTFOUND). Showing the last results; trying again in 10 minutes."}
{"type":"error","message":null}
{"type":"page","project":"p_1a2b3c4d5e","page":{"blocks":[{"type":"header","title":"Shop","subtitle":"No known vulnerabilities","icon":"shield","tone":"green"}]}}
{"type":"page","project":null,"error":"Still reading lockfiles"}
{"type":"nav","items":[{"project":null,"badge":{"text":"3 critical","tone":"red"}},{"project":"p_1a2b3c4d5e","badge":{"text":"2 critical","tone":"red"}}]}
```

| Message | Capability | Does | Fields |
| --- | --- | --- | --- |
| `signal` | `signals` | Adds or replaces a signal by `id`; `count: 0` removes it. | `id` (token), `icon`, `count` (whole number; Values above 9,999 show as 9,999), `tone`, `label` (tooltip; defaults to the plugin's name). |
| `live` | `live` | Sets the live item, or `clear: true`. The notch shows the first enabled plugin's, in Settings order. | `text`, `icon` (at least one), `tone`. Text is cut to 24 characters. |
| `widget` | `widget` | Sets the Today card, or `clear: true`. | `title` (required), `rows` (`text` required, `detail`, `icon`, `tone`), `actions` (`id` unique token, `label` required, `icon`). |
| `feed` | `feed` | Adds or replaces an Alerts item by `id`; `remove: true` drops one; `clear: true` drops all. | See below. |
| `error` | None needed | Shows a note under the plugin in Settings; `null` or `""` clears it. | `message`. |
| `page` | `tab` | A page for `project` (`null`: overview), with exactly one of `page` (`{"blocks": [...]}`) or `error` (200). | See [Pages](#pages). |
| `nav` | `tab` | Orders and badges the tab's sidebar, replacing the last `nav`. | `items[]`: `project` (`null`: **All projects**), `badge` (`{"text", "tone"}`). Unlisted projects follow in Pill's order. |

`tone` defaults to `neutral`. Too many widget rows or buttons, or feed item
buttons, refuse the message. Feed item fields:

| Field | Rules |
| --- | --- |
| `id`, `title` | An item id, required unless `clear`; a title, required to add. |
| `severity`, `icon`, `body`, `detail`, `url` | `severity` defaults to `info`; `body` is one paragraph shown when opened, `detail` one line, `url` a link. |
| `project` | One of Pill's current projects; needs `projects`. A badge that opens the plugin's tab at its [row anchor](#list-rows). |
| `task` | `{"title", "notes"}` for **Fix with an agent**; needs `project`. Adds a To do task (or finds an open one with that title); `critical` and `high` give priority 1. Notes are [fenced](#task-fencing). |
| `actions` | Widget buttons; a click sends `action` with `item`. |

Items sort by severity, then arrival, grouped by `url` or title; `critical` and
`high` add to the notch's shield. Dismissed ids stay ignored; Pill keeps the
last 2,000 dismissed ids per plugin, so use ids that change when the situation
does.

## Pages

The manifest's `tab` adds a header tab. With `projects` the tab has a sidebar
(**All projects**, then every project) and project pages also show in each
project's Chat sidebar; without it there is only the overview. When a window
starts showing a page no other window shows, Pill sends `view` and shows any
cached copy; the plugin answers with `page` and resends it when it changes.
When the last window leaves, Pill sends `view` with `closed`; after a restart
it resends `view` for every page on screen. Pill caches up to 16 pages per
plugin, evicting the least recently used one no window shows.

### Blocks

Numbers are character limits; an invalid or incomplete block, row or item is
dropped.

| Block | Fields |
| --- | --- |
| `header` | `title` (80, required), `subtitle` (160), `icon`, `tone`, `actions`. |
| `stats` | `items`: 1–4 tiles of `label` (24, required), `value` (16, required; text or number), `detail` (40), `tone`. One bad tile drops the block. |
| `section` | `title` (60, required), `count`, `collapsed`, `blocks`. No nested sections. |
| `list` | `rows`; see [List rows](#list-rows). Dropped with no valid rows. |
| `table` | `columns`: 1–6 of `label` (24, required) and `align` (`start` or `end`). `rows`: one cell per column, text or number, 80. |
| `text` | `text` (2,000, required; multi-line), `muted`. |
| `keyvalue` | `items`: 1–12 of `label` (32, required) and `value` (160, required). One bad pair drops the block. |
| `notice` | `tone`, `icon`, `title` (80), `text` (400, required), `actions`. |
| `empty` | `icon`, `title` (80, required), `text` (240), `actions`. |

### List rows

A row with an invalid or repeated `id`, an invalid `open`, or buttons but no
`id` is dropped.

| Field | Type | Rules |
| --- | --- | --- |
| `id` | string | An item id, unique on the page; required with buttons; sent as `item`. |
| `title`, `detail` | string | 120, required; 160. |
| `body` | string | Multi-line, 1,200. Makes the row expandable. |
| `severity`, `tone`, `icon` | string | From [Vocabularies](#vocabularies). |
| `chips` | object[] | Up to 4 of `{"text" (32, required), "tone"}`. |
| `url` | string | Adds an **Open** button; a task button's link. |
| `open` | string | A project id; clicking shows its page. |
| `actions` | object[] | Buttons, ids unique in the row. |

A row whose `id` equals a feed item id is that alert's anchor: the alert's
project badge opens the page, unfolds the row's sections, opens its `body` and
highlights it.

### Buttons

`{"id", "label", "icon", "primary", "url", "task"}`: `id` a token, `label`
required. A plugin button sends `action` with `page` (plus `item` on a row); a
`url` button opens the link; a `task` button (`title` required, 120; `notes`
multi-line, 4,000) adds the task to the page's project, or finds it by title,
and offers **Start agent**, with priority 1 on a `critical` or `high` row. Task
buttons don't work on the overview. Invalid or repeated buttons are dropped,
as is one with both `url` and `task`; Settings lists what a page lost
("Dropped parts of the page for p_1a2b…: …").

### Task fencing

Task notes reach agents inside `<untrusted-plugin>` tags, after a line naming
the plugin, the `Link:` and a warning not to follow instructions in them. Fence
tags inside the notes or the plugin name are broken up; empty notes become
`(no details)`.

## Vocabularies

A name outside these lists invalidates the message, block, row or button.

### Icons

`alert`, `bell`, `bot`, `branch`, `bug`, `calendar`, `chat`, `check`,
`clock`, `cloud`, `code`, `download`, `error`, `flash`, `globe`, `heart`,
`info`, `mail`, `music`, `pause`, `person`, `play`, `rocket`, `shield`,
`star`, `timer`, `warning`, `wrench`

### Tones

`neutral`, `accent`, `green`, `amber`, `red`, `purple`

### Severities

Most to least urgent: `critical`, `high`, `medium`, `low`, `info`

## Limits

| Limit | Value | What happens |
| --- | --- | --- |
| Manifest size | 64 KB | The plugin shows as invalid. |
| Plugin folders | 32 | Later folders are ignored. |
| Line length | 8 KB | The line is refused. |
| Page line length | 128 KB | `page` and `nav` only; the line is skipped. |
| Rate | 50 lines per second | The rest of that second is dropped. |
| Signals | 4 per plugin | New ids are refused. |
| Widget rows / buttons | 8 / 4 | The widget is refused. |
| Feed items | 100 per plugin | New ids are refused. |
| Feed item buttons | 2 | The item is refused. |
| Dismissed feed ids | 2000 per plugin | The oldest are forgotten. |
| Blocks | 120 per page, sections included | Later blocks are dropped. |
| Rows | 100 per list or table, 400 per page | Later rows are dropped. |
| Block and row buttons | 3 | Later buttons are dropped. |
| Sidebar entries | 256 per `nav` | Later entries are dropped. |
| Cached pages | 16 per plugin | The least recently used unseen page goes. |
| Plugin tabs in the header | 3 | The rest open from the `+N` menu. |

Text limits, in characters: live text 24, signal label 60, widget title 40,
widget row 80, widget button label 20, feed title 120, feed body 400, feed
detail 120, task title 120, task notes 4,000, page button label 24, error 200,
badge 16, tab title 16.

## Status and errors in Settings

A plugin is **Off**, **Starting**, **Running** (with its `error` or Pill's
latest note), **Error** (couldn't start or stopped on its own; has
**Restart**), **Changed** (the approval string no longer matches) or
**Invalid** (broken manifest).

| Note | Cause |
| --- | --- |
| `Ignored a line: not a valid message: …` | Not JSON, an unknown `type`, a missing field, or an unknown icon, tone or severity. |
| `Ignored a line: line is N bytes; the limit is 8192` | A non-page line over 8 KB. |
| `Ignored a N-byte line (limit 131072)` | Any line over 128 KB. |
| `Ignored a line: … isn't in this plugin's capabilities` | An undeclared capability. |
| `Ignored a line: at most 4 signals at a time` | A fifth signal id. |
| `Ignored a line: at most 100 feed items at a time` | A 101st feed id. |
| `Ignored a line: pages for a project need the projects capability` | A project page without `projects`. |
| `Ignored a line: feed items for a project need the projects capability` | A feed `project` without `projects`. |
| `Ignored a line: project "…" isn't one of Pill's projects` | A feed `project` that doesn't exist. |
| `Too chatty: ignored N lines (limit 50 per second)` | The rate limit. |
| `Dropped parts of …: …` | Parts of a page or sidebar were invalid. |

Other refusals name the field (`widget has 9 rows; the limit is 8`). Pill also
logs them to `%LOCALAPPDATA%\dev.pill.app\logs`.

## Versioning and safety

Version `1` changes only by addition; check `pill` in `hello` before relying
on something newer, and report an `error` when `protocol` isn't `1`. A plugin
runs as you and can read your files and use the network: install only plugins
you would run yourself. To test one, pipe Pill's messages into it with
`PILL_PLUGIN_DATA` set to a scratch folder.
