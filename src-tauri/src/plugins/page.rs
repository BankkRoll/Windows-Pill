//! Plugin pages: what a plugin with the `tab` capability shows in its tab.
//!
//! Responsibilities:
//! - Parse a `page` message's blocks into the typed [`Page`] the UI renders
//!   with Pill's own components, enforcing sizes, counts, nesting depth,
//!   string lengths and the fixed vocabularies.
//! - Drop a bad block, row or button and keep the rest, collecting a short
//!   warning for each so the author can see what was refused.
//! - Find the button a click refers to, so Pill only sends actions (and only
//!   creates tasks) a cached page actually offers.
//! - Parse `nav` messages: the badge and order of each entry in the tab's
//!   project sidebar. The entries themselves are Pill's own projects.
//!
//! SECURITY: like every plugin output, a page is data, never markup. Text is
//! cleaned with [`clean_text`] / [`clean_block`], icons, tones and severities
//! come from fixed lists, and the only links are plain `https://` URLs that
//! open when the user clicks them.

use super::protocol::{clean_block, clean_text, clean_url, is_item_id, is_token, Icon, Severity, Tone, MAX_TASK_NOTES, MAX_TASK_TITLE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest `page` line Pill accepts, in bytes. Every other message keeps
/// the 8 KB limit.
pub const MAX_PAGE_LINE_BYTES: usize = 128 * 1024;
/// Blocks in one page, counting those inside sections.
pub const MAX_BLOCKS: usize = 120;
/// List and table rows in one page, all blocks together.
pub const MAX_PAGE_ROWS: usize = 400;
/// Rows in one list block.
pub const MAX_LIST_ROWS: usize = 100;
/// Rows in one table block.
pub const MAX_TABLE_ROWS: usize = 100;
/// Columns in one table block.
pub const MAX_COLUMNS: usize = 6;
/// Tiles in one stats block.
pub const MAX_STATS: usize = 4;
/// Pairs in one keyvalue block.
pub const MAX_PAIRS: usize = 12;
/// Chips on one list row.
pub const MAX_CHIPS: usize = 4;
/// Buttons on one list row, header, notice or empty block.
pub const MAX_BLOCK_ACTIONS: usize = 3;
/// Characters of a button label.
pub const MAX_BUTTON_LABEL: usize = 24;
/// Characters of a list row's expandable body.
pub const MAX_ROW_BODY: usize = 1_200;
/// Characters of a text block.
pub const MAX_TEXT: usize = 2_000;
/// Characters of a tab title in the manifest.
pub const MAX_TAB_TITLE: usize = 16;
/// Pages Pill keeps per plugin; the least recently used goes first.
pub const MAX_CACHED_PAGES: usize = 16;
/// Characters of a page error.
pub const MAX_PAGE_ERROR: usize = 200;
/// Entries in one `nav` message.
pub const MAX_NAV_ITEMS: usize = 256;
/// Characters of a sidebar badge.
pub const MAX_BADGE: usize = 16;
/// Warnings kept per page; the rest are counted.
const MAX_WARNINGS: usize = 8;

/// A plugin's tab in Pill's header, from the manifest's `tab` field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabSpec {
    /// 1–16 characters on one line, e.g. `Security`.
    pub title: String,
    pub icon: Icon,
}

/// What a page button does when clicked.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ButtonKind {
    /// Sends `{"type":"action",…,"page":{"project":…}}` to the plugin.
    Plugin,
    /// Pill adds the suggested task to the page's project and offers to
    /// start an agent on it; the plugin isn't told.
    Task { task: SuggestedTask },
    /// Opens a plain `https://` link in the browser.
    Link { url: String },
}

/// A task a page button suggests.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedTask {
    pub title: String,
    /// Kept out of the UI state; Pill fences them as untrusted text when it
    /// writes the task.
    #[serde(skip_serializing)]
    pub notes: String,
}

/// A button on a page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    /// Unique among the buttons it is checked against: a row's buttons, or
    /// every header, notice and empty block button on the page.
    pub id: String,
    pub label: String,
    pub icon: Option<Icon>,
    /// Drawn in the accent color; one per group is plenty.
    pub primary: bool,
    #[serde(flatten)]
    pub kind: ButtonKind,
}

/// One tile of a stats block.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub label: String,
    /// Short, usually a number: `12`, `3.1k`, `OK`.
    pub value: String,
    pub detail: Option<String>,
    pub tone: Tone,
}

/// A small label on a list row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chip {
    pub text: String,
    pub tone: Tone,
}

/// One row of a list block.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// Sent as `item` with the row's plugin buttons; required when the row
    /// has buttons, unique within the page.
    pub id: Option<String>,
    pub title: String,
    pub detail: Option<String>,
    /// Paragraphs shown when the row is opened.
    pub body: Option<String>,
    pub severity: Option<Severity>,
    pub tone: Tone,
    pub icon: Option<Icon>,
    pub chips: Vec<Chip>,
    /// An `https://` link the row offers to open.
    pub url: Option<String>,
    /// A project id: clicking the row shows that project's page.
    pub open: Option<String>,
    pub actions: Vec<Button>,
}

/// How a table column's cells line up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Start,
    End,
}

/// A table column.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub label: String,
    pub align: Align,
}

/// A label and its value.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub label: String,
    pub value: String,
}

/// One block of a page. Sections hold other blocks, but not sections.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    /// The page's title line, with an overall status.
    Header { title: String, subtitle: Option<String>, icon: Option<Icon>, tone: Tone, actions: Vec<Button> },
    /// Two to four number tiles.
    Stats { items: Vec<Stat> },
    /// A titled group of blocks that the user can fold.
    Section { title: String, count: Option<u32>, collapsed: bool, blocks: Vec<Block> },
    /// Rows with a severity or tone, chips, an expandable body and buttons.
    List { rows: Vec<Row> },
    /// Plain-text columns and rows.
    Table { columns: Vec<Column>, rows: Vec<Vec<String>> },
    /// Plain paragraphs.
    Text { text: String, muted: bool },
    /// Label and value pairs.
    #[serde(rename = "keyvalue")]
    KeyValue { items: Vec<Pair> },
    /// A colored callout with optional buttons.
    Notice { tone: Tone, icon: Option<Icon>, title: Option<String>, text: String, actions: Vec<Button> },
    /// What to show when there is nothing to show.
    Empty { icon: Option<Icon>, title: String, text: Option<String>, actions: Vec<Button> },
}

/// A validated page.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub blocks: Vec<Block>,
}

/// What a `page` message carries: a page, or why the plugin couldn't make one.
#[derive(Debug, Clone, PartialEq)]
pub enum PageContent {
    Page(Page),
    Error(String),
}

/// A validated `page` message.
#[derive(Debug, Clone, PartialEq)]
pub struct PageUpdate {
    /// The project the page is about; `None` for the overview page.
    pub project: Option<String>,
    pub content: PageContent,
    /// What was dropped while parsing, for the plugin's status line.
    pub warnings: Vec<String>,
}

/// A project's entry in a tab's sidebar, as the plugin describes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavItem {
    /// The project; `None` for the "All projects" entry.
    pub project: Option<String>,
    /// A short count or state, e.g. `3 critical`.
    pub badge: Option<Chip>,
}

/// A button found by [`Page::find_button`], with the row it sits on.
pub struct Found<'a> {
    pub button: &'a Button,
    pub row: Option<&'a Row>,
}

impl Page {
    /// Finds the button a click names. Row buttons are found by the row's
    /// id; header, notice and empty block buttons by their own id alone.
    ///
    /// @param item The row id, if the button is on a row.
    /// @param id The button id.
    pub fn find_button(&self, item: Option<&str>, id: &str) -> Option<Found<'_>> {
        find_in(&self.blocks, item, id)
    }
}

fn find_in<'a>(blocks: &'a [Block], item: Option<&str>, id: &str) -> Option<Found<'a>> {
    for block in blocks {
        let found = match (block, item) {
            (Block::Section { blocks, .. }, _) => find_in(blocks, item, id),
            (Block::List { rows }, Some(item)) => rows
                .iter()
                .find(|r| r.id.as_deref() == Some(item))
                .and_then(|row| row.actions.iter().find(|b| b.id == id).map(|button| Found { button, row: Some(row) })),
            (Block::Header { actions, .. } | Block::Notice { actions, .. } | Block::Empty { actions, .. }, None) => {
                actions.iter().find(|b| b.id == id).map(|button| Found { button, row: None })
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

// -----------------------------------------------------------------------------
// Parsing
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawButton {
    id: String,
    label: String,
    #[serde(default)]
    icon: Option<Icon>,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    task: Option<RawTask>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct RawTask {
    title: String,
    #[serde(default)]
    notes: String,
}

#[derive(Deserialize)]
struct RawStat {
    label: String,
    value: Value,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    tone: Tone,
}

#[derive(Deserialize)]
struct RawChip {
    text: String,
    #[serde(default)]
    tone: Tone,
}

#[derive(Deserialize)]
struct RawRow {
    #[serde(default)]
    id: Option<String>,
    title: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    severity: Option<Severity>,
    #[serde(default)]
    tone: Tone,
    #[serde(default)]
    icon: Option<Icon>,
    #[serde(default)]
    chips: Vec<Value>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    open: Option<String>,
    #[serde(default)]
    actions: Vec<Value>,
}

#[derive(Deserialize)]
struct RawColumn {
    label: String,
    #[serde(default)]
    align: Align,
}

#[derive(Deserialize)]
struct RawPair {
    label: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum RawBlock {
    Header {
        title: String,
        #[serde(default)]
        subtitle: Option<String>,
        #[serde(default)]
        icon: Option<Icon>,
        #[serde(default)]
        tone: Tone,
        #[serde(default)]
        actions: Vec<Value>,
    },
    Stats {
        items: Vec<Value>,
    },
    Section {
        title: String,
        #[serde(default)]
        count: Option<u32>,
        #[serde(default)]
        collapsed: bool,
        #[serde(default)]
        blocks: Vec<Value>,
    },
    List {
        rows: Vec<Value>,
    },
    Table {
        columns: Vec<Value>,
        rows: Vec<Value>,
    },
    Text {
        text: String,
        #[serde(default)]
        muted: bool,
    },
    #[serde(rename = "keyvalue")]
    KeyValue {
        items: Vec<Value>,
    },
    Notice {
        #[serde(default)]
        tone: Tone,
        #[serde(default)]
        icon: Option<Icon>,
        #[serde(default)]
        title: Option<String>,
        text: String,
        #[serde(default)]
        actions: Vec<Value>,
    },
    Empty {
        #[serde(default)]
        icon: Option<Icon>,
        title: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        actions: Vec<Value>,
    },
}

/// Running totals and what was dropped while one page is parsed.
struct Parser {
    /// The page's project; task buttons need one.
    has_project: bool,
    blocks: usize,
    rows: usize,
    /// Row ids seen so far, which must be unique.
    row_ids: Vec<String>,
    /// Header, notice and empty block button ids seen so far.
    block_buttons: Vec<String>,
    warnings: Vec<String>,
    dropped: usize,
}

impl Parser {
    fn warn(&mut self, what: impl Into<String>) {
        self.dropped += 1;
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(what.into());
        }
    }

    fn blocks(&mut self, raw: &[Value], nested: bool) -> Vec<Block> {
        let mut out = Vec::new();
        for (i, v) in raw.iter().enumerate() {
            if self.blocks >= MAX_BLOCKS {
                self.warn(format!("blocks after the first {MAX_BLOCKS} (dropped {})", raw.len() - i));
                break;
            }
            match self.block(v, nested) {
                Ok(block) => {
                    self.blocks += 1;
                    out.push(block);
                }
                Err(e) => {
                    let kind = v.get("type").and_then(Value::as_str).map(|t| clean_text(t, 16)).unwrap_or_else(|| "?".into());
                    self.warn(format!("{kind} block: {e}"));
                }
            }
        }
        out
    }

    fn block(&mut self, v: &Value, nested: bool) -> Result<Block, String> {
        let raw: RawBlock = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        Ok(match raw {
            RawBlock::Header { title, subtitle, icon, tone, actions } => Block::Header {
                title: required(&title, 80, "title")?,
                subtitle: optional(subtitle.as_deref(), 160),
                icon,
                tone,
                actions: self.block_buttons(&actions),
            },
            RawBlock::Stats { items } => {
                if items.is_empty() || items.len() > MAX_STATS {
                    return Err(format!("needs 1-{MAX_STATS} items"));
                }
                let items = items
                    .iter()
                    .map(|v| {
                        let s: RawStat = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
                        Ok(Stat {
                            label: required(&s.label, 24, "label")?,
                            value: required(&scalar(&s.value)?, 16, "value")?,
                            detail: optional(s.detail.as_deref(), 40),
                            tone: s.tone,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Block::Stats { items }
            }
            RawBlock::Section { .. } if nested => return Err("sections can't hold sections".into()),
            RawBlock::Section { title, count, collapsed, blocks } => {
                let title = required(&title, 60, "title")?;
                Block::Section { title, count, collapsed, blocks: self.blocks(&blocks, true) }
            }
            RawBlock::List { rows } => {
                let rows = self.list_rows(&rows);
                if rows.is_empty() {
                    return Err("has no valid rows".into());
                }
                Block::List { rows }
            }
            RawBlock::Table { columns, rows } => self.table(&columns, &rows)?,
            RawBlock::Text { text, muted } => Block::Text { text: required_block(&text, MAX_TEXT)?, muted },
            RawBlock::KeyValue { items } => {
                if items.is_empty() || items.len() > MAX_PAIRS {
                    return Err(format!("needs 1-{MAX_PAIRS} items"));
                }
                let items = items
                    .iter()
                    .map(|v| {
                        let p: RawPair = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
                        Ok(Pair { label: required(&p.label, 32, "label")?, value: required(&scalar(&p.value)?, 160, "value")? })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Block::KeyValue { items }
            }
            RawBlock::Notice { tone, icon, title, text, actions } => Block::Notice {
                tone,
                icon,
                title: optional(title.as_deref(), 80),
                text: required(&text, 400, "text")?,
                actions: self.block_buttons(&actions),
            },
            RawBlock::Empty { icon, title, text, actions } => Block::Empty {
                icon,
                title: required(&title, 80, "title")?,
                text: optional(text.as_deref(), 240),
                actions: self.block_buttons(&actions),
            },
        })
    }

    fn list_rows(&mut self, raw: &[Value]) -> Vec<Row> {
        let mut rows = Vec::new();
        for (i, v) in raw.iter().enumerate() {
            if rows.len() >= MAX_LIST_ROWS || self.rows >= MAX_PAGE_ROWS {
                self.warn(format!("list rows over the limit (dropped {})", raw.len() - i));
                break;
            }
            match self.row(v) {
                Ok(row) => {
                    self.rows += 1;
                    rows.push(row);
                }
                Err(e) => self.warn(format!("list row: {e}")),
            }
        }
        rows
    }

    fn row(&mut self, v: &Value) -> Result<Row, String> {
        let r: RawRow = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        let title = required(&r.title, 120, "title")?;
        if let Some(p) = r.open.as_deref().filter(|p| !super::protocol::is_project_id(p)) {
            return Err(format!("open {:?} isn't a project id", clean_text(p, 40)));
        }
        let id = match r.id {
            Some(id) if !is_item_id(&id) => return Err(format!("id {:?} isn't a valid item id", clean_text(&id, 40))),
            Some(id) if self.row_ids.contains(&id) => return Err(format!("id {id:?} appears twice")),
            id => id,
        };
        let mut actions = Vec::new();
        for a in r.actions.iter() {
            if actions.len() >= MAX_BLOCK_ACTIONS {
                self.warn(format!("row {title:?}: more than {MAX_BLOCK_ACTIONS} buttons"));
                break;
            }
            match self.button(a) {
                Ok(b) if actions.iter().any(|x: &Button| x.id == b.id) => self.warn(format!("button id {:?} appears twice on a row", b.id)),
                Ok(b) => actions.push(b),
                Err(e) => self.warn(format!("row button: {e}")),
            }
        }
        if id.is_none() && actions.iter().any(|b| !matches!(b.kind, ButtonKind::Link { .. })) {
            return Err("a row with buttons needs an id".into());
        }
        let mut chips = Vec::new();
        for c in r.chips.iter().take(MAX_CHIPS) {
            match serde_json::from_value::<RawChip>(c.clone()) {
                Ok(c) => match optional(Some(&c.text), 32) {
                    Some(text) => chips.push(Chip { text, tone: c.tone }),
                    None => self.warn("chip: empty text"),
                },
                Err(e) => self.warn(format!("chip: {e}")),
            }
        }
        let url = match r.url.as_deref().map(clean_url).transpose() {
            Ok(url) => url,
            Err(e) => {
                self.warn(format!("row {title:?}: {e}"));
                None
            }
        };
        if let Some(id) = &id {
            self.row_ids.push(id.clone());
        }
        Ok(Row {
            id,
            title,
            detail: optional(r.detail.as_deref(), 160),
            body: r.body.as_deref().map(|b| clean_block(b, MAX_ROW_BODY)).filter(|b| !b.is_empty()),
            severity: r.severity,
            tone: r.tone,
            icon: r.icon,
            chips,
            url,
            open: r.open,
            actions,
        })
    }

    fn table(&mut self, columns: &[Value], rows: &[Value]) -> Result<Block, String> {
        if columns.is_empty() || columns.len() > MAX_COLUMNS {
            return Err(format!("needs 1-{MAX_COLUMNS} columns"));
        }
        let columns = columns
            .iter()
            .map(|v| {
                let c: RawColumn = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
                Ok(Column { label: required(&c.label, 24, "column label")?, align: c.align })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut out = Vec::new();
        for (i, v) in rows.iter().enumerate() {
            if out.len() >= MAX_TABLE_ROWS || self.rows >= MAX_PAGE_ROWS {
                self.warn(format!("table rows over the limit (dropped {})", rows.len() - i));
                break;
            }
            let cells = v.as_array().map(|cells| cells.iter().map(scalar).collect::<Result<Vec<_>, String>>());
            match cells {
                Some(Ok(cells)) if cells.len() == columns.len() => {
                    self.rows += 1;
                    out.push(cells.iter().map(|c| clean_text(c, 80)).collect());
                }
                Some(Ok(cells)) => self.warn(format!("table row has {} cells for {} columns", cells.len(), columns.len())),
                Some(Err(e)) => self.warn(format!("table cell: {e}")),
                None => self.warn("table row isn't a list of cells"),
            }
        }
        Ok(Block::Table { columns, rows: out })
    }

    /// Header, notice and empty block buttons, whose ids are unique across
    /// the page.
    fn block_buttons(&mut self, raw: &[Value]) -> Vec<Button> {
        let mut out = Vec::new();
        for v in raw {
            if out.len() >= MAX_BLOCK_ACTIONS {
                self.warn(format!("more than {MAX_BLOCK_ACTIONS} buttons on a block"));
                break;
            }
            match self.button(v) {
                Ok(b) if self.block_buttons.contains(&b.id) => self.warn(format!("button id {:?} appears twice", b.id)),
                Ok(b) => {
                    self.block_buttons.push(b.id.clone());
                    out.push(b);
                }
                Err(e) => self.warn(format!("button: {e}")),
            }
        }
        out
    }

    fn button(&self, v: &Value) -> Result<Button, String> {
        let b: RawButton = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        if !is_token(&b.id) {
            return Err(format!("id {:?} must be 1-32 characters of a-z, 0-9, - or _", clean_text(&b.id, 40)));
        }
        let label = required(&b.label, MAX_BUTTON_LABEL, "label")?;
        let kind = match (b.task, b.url) {
            (Some(_), Some(_)) => return Err(format!("{:?} can't both add a task and open a link", b.id)),
            (Some(_), None) if !self.has_project => return Err(format!("{:?}: task buttons only work on a project's page", b.id)),
            (Some(t), None) => ButtonKind::Task {
                task: SuggestedTask { title: required(&t.title, MAX_TASK_TITLE, "task title")?, notes: clean_block(&t.notes, MAX_TASK_NOTES) },
            },
            (None, Some(url)) => ButtonKind::Link { url: clean_url(&url)? },
            (None, None) => ButtonKind::Plugin,
        };
        Ok(Button { id: b.id, label, icon: b.icon, primary: b.primary, kind })
    }
}

/// A string, or a number written as text, from a value plugins may send as
/// either (stat values, table cells, pairs).
fn scalar(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err("expected text or a number".into()),
    }
}

fn required(s: &str, max: usize, what: &str) -> Result<String, String> {
    let s = clean_text(s, max);
    if s.is_empty() {
        Err(format!("{what} is empty"))
    } else {
        Ok(s)
    }
}

fn required_block(s: &str, max: usize) -> Result<String, String> {
    let s = clean_block(s, max);
    if s.is_empty() {
        Err("text is empty".into())
    } else {
        Ok(s)
    }
}

fn optional(s: Option<&str>, max: usize) -> Option<String> {
    s.map(|s| clean_text(s, max)).filter(|s| !s.is_empty())
}

/// Parses a page's `blocks`, dropping what is invalid.
///
/// @param page The message's `page` object.
/// @param has_project Whether the page is about a project (task buttons need one).
/// @returns The page and what was dropped, or why the whole page was refused.
pub fn parse_page(page: &Value, has_project: bool) -> Result<(Page, Vec<String>), String> {
    let blocks = page.get("blocks").and_then(Value::as_array).ok_or("a page needs \"blocks\": […]")?;
    let mut p = Parser { has_project, blocks: 0, rows: 0, row_ids: Vec::new(), block_buttons: Vec::new(), warnings: Vec::new(), dropped: 0 };
    let blocks = p.blocks(blocks, false);
    if p.dropped > p.warnings.len() {
        let more = p.dropped - p.warnings.len();
        p.warnings.push(format!("and {more} more"));
    }
    Ok((Page { blocks }, p.warnings))
}

#[derive(Deserialize)]
struct RawNavItem {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    badge: Option<RawChip>,
}

/// Parses a `nav` message's `items`, dropping invalid and repeated
/// entries. Their order is the order the sidebar lists projects in.
///
/// @param items The message's `items`.
/// @returns The entries and what was dropped.
pub fn parse_nav(items: &[Value]) -> (Vec<NavItem>, Vec<String>) {
    let mut out: Vec<NavItem> = Vec::new();
    let mut warnings = Vec::new();
    if items.len() > MAX_NAV_ITEMS {
        warnings.push(format!("nav entries after the first {MAX_NAV_ITEMS}"));
    }
    for v in items.iter().take(MAX_NAV_ITEMS) {
        let item = match serde_json::from_value::<RawNavItem>(v.clone()) {
            Ok(item) => item,
            Err(e) => {
                warnings.push(format!("nav entry: {e}"));
                continue;
            }
        };
        if let Some(p) = item.project.as_deref().filter(|p| !super::protocol::is_project_id(p)) {
            warnings.push(format!("nav entry: {:?} isn't a project id", clean_text(p, 40)));
            continue;
        }
        if out.iter().any(|x| x.project == item.project) {
            warnings.push("nav entry: a project appears twice".into());
            continue;
        }
        let badge = item.badge.and_then(|b| optional(Some(&b.text), MAX_BADGE).map(|text| Chip { text, tone: b.tone }));
        out.push(NavItem { project: item.project, badge });
    }
    warnings.truncate(MAX_WARNINGS);
    (out, warnings)
}

/// Validates a manifest's `tab` field.
///
/// @param title The tab's title.
/// @param icon Its icon.
pub fn tab_spec(title: &str, icon: Icon) -> Result<TabSpec, String> {
    let clean = title.trim();
    if clean.is_empty() || clean.chars().count() > MAX_TAB_TITLE || clean.chars().any(char::is_control) {
        return Err(format!("tab title must be 1-{MAX_TAB_TITLE} characters on one line"));
    }
    Ok(TabSpec { title: clean.to_string(), icon })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(page: Value, has_project: bool) -> (Page, Vec<String>) {
        parse_page(&page, has_project).unwrap()
    }

    #[test]
    fn parses_every_block_type() {
        let (page, warnings) = parse(
            json!({ "blocks": [
                { "type": "header", "title": "Shop", "subtitle": "2 critical", "icon": "shield", "tone": "red",
                  "actions": [{ "id": "check-now", "label": "Check now" }] },
                { "type": "stats", "items": [{ "label": "Critical", "value": 2, "tone": "red" }, { "label": "Packages", "value": "412" }] },
                { "type": "section", "title": "Upgrade plan", "count": 1, "blocks": [
                    { "type": "list", "rows": [{ "id": "npm/next", "title": "Upgrade next", "severity": "critical",
                      "chips": [{ "text": "direct", "tone": "accent" }], "url": "https://osv.dev/list",
                      "actions": [{ "id": "fix", "label": "Fix with an agent", "primary": true, "task": { "title": "Upgrade next", "notes": "a\nb" } },
                                  { "id": "ignore", "label": "Ignore package" }] }] }
                ] },
                { "type": "table", "columns": [{ "label": "File" }, { "label": "Packages", "align": "end" }], "rows": [["package-lock.json", 412]] },
                { "type": "text", "text": "One.\n\nTwo.", "muted": true },
                { "type": "keyvalue", "items": [{ "label": "Last checked", "value": "14:05" }] },
                { "type": "notice", "tone": "amber", "text": "Offline", "actions": [{ "id": "fix-all", "label": "Fix everything", "task": { "title": "Fix all" } }] },
                { "type": "empty", "title": "Nothing found", "actions": [{ "id": "docs", "label": "Docs", "url": "https://example.com/docs" }] }
            ] }),
            true,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(page.blocks.len(), 8);
        let found = page.find_button(Some("npm/next"), "fix").unwrap();
        assert!(matches!(&found.button.kind, ButtonKind::Task { task } if task.notes == "a\nb"));
        assert_eq!(found.row.unwrap().url.as_deref(), Some("https://osv.dev/list"));
        assert!(page.find_button(None, "check-now").is_some());
        assert!(page.find_button(None, "fix-all").is_some());
        assert!(page.find_button(None, "fix").is_none(), "row buttons need their row");
        assert!(page.find_button(Some("npm/other"), "fix").is_none());
        let v = serde_json::to_value(&page).unwrap();
        assert_eq!(v["blocks"][1]["items"][0]["value"], "2");
        assert_eq!(v["blocks"][5]["type"], "keyvalue");
        let button = &v["blocks"][2]["blocks"][0]["rows"][0]["actions"][0];
        assert_eq!(button["kind"], "task");
        assert_eq!(button["task"], json!({ "title": "Upgrade next" }), "task notes stay out of the UI state");
        assert_eq!(v["blocks"][7]["actions"][0]["url"], "https://example.com/docs");
    }

    #[test]
    fn bad_blocks_are_dropped_and_good_ones_kept() {
        let (page, warnings) = parse(
            json!({ "blocks": [
                { "type": "html", "html": "<script>" },
                { "type": "header", "title": "  " },
                { "type": "stats", "items": [] },
                { "type": "text", "text": "kept" },
                { "type": "header", "title": "T", "tone": "rainbow" },
                { "type": "section", "title": "Outer", "blocks": [{ "type": "section", "title": "Inner", "blocks": [] }, { "type": "text", "text": "in" }] },
                "not an object"
            ] }),
            false,
        );
        assert_eq!(page.blocks.len(), 2);
        assert!(matches!(&page.blocks[1], Block::Section { blocks, .. } if blocks.len() == 1));
        assert_eq!(warnings.len(), 6, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("sections can't hold sections")));
        assert!(parse_page(&json!({ "nope": [] }), false).is_err());
    }

    #[test]
    fn rows_and_buttons_are_validated_one_by_one() {
        let (page, warnings) = parse(
            json!({ "blocks": [{ "type": "list", "rows": [
                { "id": "a", "title": "Kept", "url": "javascript:alert(1)", "actions": [
                    { "id": "ok", "label": "OK" }, { "id": "ok", "label": "Twice" }, { "id": "Bad Id", "label": "x" },
                    { "id": "t", "label": "Task", "task": { "title": "Fix" } }, { "id": "both", "label": "B", "task": { "title": "x" }, "url": "https://a.example" } ] },
                { "id": "a", "title": "Duplicate id" },
                { "title": "Buttons without an id", "actions": [{ "id": "x", "label": "X" }] },
                { "title": "Link only, no id needed", "actions": [{ "id": "open", "label": "Open", "url": "https://osv.dev/" }] },
                { "id": "has space", "title": "Bad id" },
                { "title": "Opens a project", "open": "p_1" },
                { "title": "Opens nowhere", "open": "../etc" }
            ] }] }),
            false,
        );
        let Block::List { rows } = &page.blocks[0] else { panic!() };
        assert_eq!(rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Kept", "Link only, no id needed", "Opens a project"]);
        assert_eq!(rows[2].open.as_deref(), Some("p_1"));
        assert_eq!(rows[0].url, None, "only https links");
        assert_eq!(rows[0].actions.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["ok"], "task buttons need a project page");
        assert!(warnings.iter().any(|w| w.contains("only work on a project's page")));
        assert!(warnings.iter().any(|w| w.contains("appears twice")));
    }

    #[test]
    fn limits_hold() {
        let rows: Vec<Value> = (0..MAX_LIST_ROWS + 5).map(|i| json!({ "title": format!("r{i}") })).collect();
        let (page, warnings) = parse(json!({ "blocks": [{ "type": "list", "rows": rows }] }), false);
        let Block::List { rows } = &page.blocks[0] else { panic!() };
        assert_eq!(rows.len(), MAX_LIST_ROWS);
        assert_eq!(warnings.len(), 1);

        let blocks: Vec<Value> = (0..MAX_BLOCKS + 3).map(|i| json!({ "type": "text", "text": format!("b{i}") })).collect();
        let (page, _) = parse(json!({ "blocks": blocks }), false);
        assert_eq!(page.blocks.len(), MAX_BLOCKS);

        let lists: Vec<Value> = (0..5).map(|_| json!({ "type": "list", "rows": (0..MAX_LIST_ROWS).map(|i| json!({ "title": i.to_string() })).collect::<Vec<_>>() })).collect();
        let (page, _) = parse(json!({ "blocks": lists }), false);
        let total: usize = page.blocks.iter().map(|b| if let Block::List { rows } = b { rows.len() } else { 0 }).sum();
        assert_eq!(total, MAX_PAGE_ROWS);

        let (page, warnings) = parse(
            json!({ "blocks": [{ "type": "table", "columns": [{ "label": "A" }, { "label": "B" }], "rows": [["1", "2"], ["only one"], [{ "x": 1 }, "2"], "row"] }] }),
            false,
        );
        let Block::Table { rows, .. } = &page.blocks[0] else { panic!() };
        assert_eq!(rows.len(), 1);
        assert_eq!(warnings.len(), 3);
        let five = json!({ "blocks": [{ "type": "stats", "items": (0..5).map(|i| json!({ "label": "L", "value": i })).collect::<Vec<_>>() }] });
        assert!(parse(five, false).0.blocks.is_empty());
    }

    #[test]
    fn text_is_cleaned_and_cut() {
        let (page, _) = parse(
            json!({ "blocks": [
                { "type": "header", "title": format!("{}\u{202e}", "x".repeat(200)) },
                { "type": "list", "rows": [{ "title": "t", "body": format!("a\r\n\n\n{}", "b".repeat(MAX_ROW_BODY * 2)) }] }
            ] }),
            false,
        );
        let Block::Header { title, .. } = &page.blocks[0] else { panic!() };
        assert_eq!(title.chars().count(), 80);
        let Block::List { rows } = &page.blocks[1] else { panic!() };
        let body = rows[0].body.as_deref().unwrap();
        assert!(body.starts_with("a\n\nb") && body.chars().count() == MAX_ROW_BODY);
    }

    #[test]
    fn nav_entries_are_validated() {
        let (items, warnings) = parse_nav(&[
            json!({ "project": null, "badge": { "text": "5 critical", "tone": "red" } }),
            json!({ "project": "p_2", "badge": { "text": "a very long badge text indeed" } }),
            json!({ "project": "p_1" }),
            json!({ "project": "p_1", "badge": { "text": "again" } }),
            json!({ "project": "../x" }),
            json!({ "project": "p_3", "badge": { "text": "x", "tone": "rainbow" } }),
        ]);
        assert_eq!(items.iter().map(|i| i.project.as_deref().unwrap_or("all")).collect::<Vec<_>>(), ["all", "p_2", "p_1"]);
        assert_eq!(items[0].badge, Some(Chip { text: "5 critical".into(), tone: Tone::Red }));
        assert_eq!(items[1].badge.as_ref().unwrap().text.chars().count(), MAX_BADGE);
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn tab_specs_are_short_single_lines() {
        assert_eq!(tab_spec(" Security ", Icon::Shield).unwrap().title, "Security");
        assert!(tab_spec("", Icon::Shield).is_err());
        assert!(tab_spec("A\nB", Icon::Shield).is_err());
        assert!(tab_spec(&"x".repeat(MAX_TAB_TITLE + 1), Icon::Shield).is_err());
    }
}
