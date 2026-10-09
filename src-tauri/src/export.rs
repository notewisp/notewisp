//! Markdown export: one file per local day, each note a `## HH:MM` section.
//! Converts from the ProseMirror JSON (the source of truth). Colour and
//! highlight are dropped; everything else the editor supports is kept.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;

use crate::db;

/// Writes every live note to `dir` (created if needed). Returns files written.
pub fn export_markdown(conn: &Connection, dir: &Path) -> Result<u32, String> {
    let rows = db::export_rows(conn).map_err(|e| e.to_string())?;
    let mut days: BTreeMap<String, String> = BTreeMap::new();
    for row in rows {
        let json: Value = serde_json::from_str(&row.json).map_err(|e| e.to_string())?;
        let file = days.entry(row.day.clone()).or_insert_with(|| format!("# {}\n", row.day));
        file.push_str(&format!("\n## {}\n\n", row.time));
        let body = to_markdown(&json);
        file.push_str(body.trim_end());
        file.push('\n');
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (day, content) in &days {
        std::fs::write(dir.join(format!("{day}.md")), content).map_err(|e| e.to_string())?;
    }
    Ok(days.len() as u32)
}

/// Converts a ProseMirror document to Markdown.
pub fn to_markdown(doc: &Value) -> String {
    blocks(children(doc))
}

fn children(node: &Value) -> &[Value] {
    node.get("content").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn kind(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

fn attr<'a>(node: &'a Value, name: &str) -> Option<&'a Value> {
    node.get("attrs").and_then(|a| a.get(name)).filter(|v| !v.is_null())
}

/// Block nodes separated by blank lines.
fn blocks(nodes: &[Value]) -> String {
    nodes.iter().map(block).filter(|b| !b.is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// Escapes paragraph lines that would otherwise read as Markdown block syntax:
/// a typed "## x" must stay text (and must not look like a note's section heading).
fn escape_line_starts(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let hashes = trimmed.chars().take_while(|&c| c == '#').count();
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            let rest = &trimmed[hashes.max(digits).min(trimmed.len())..];
            let block_like = (1..=6).contains(&hashes) && (rest.is_empty() || rest.starts_with(' '))
                || ["- ", "+ ", "> "].iter().any(|p| trimmed.starts_with(p))
                || trimmed == "-"
                || trimmed == ">"
                || trimmed.starts_with("---")
                || trimmed.starts_with("===")
                || (digits > 0 && (rest.starts_with(". ") || rest.starts_with(") ")));
            if !block_like {
                line.to_owned()
            } else if digits > 0 {
                format!("{indent}{}\\{}", &trimmed[..digits], &trimmed[digits..])
            } else {
                format!("{indent}\\{trimmed}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn block(node: &Value) -> String {
    match kind(node) {
        "paragraph" => escape_line_starts(&inline(children(node))),
        "heading" => {
            // Note headings sit two levels below the `## HH:MM` section heading.
            let level = attr(node, "level").and_then(Value::as_u64).unwrap_or(1).clamp(1, 4) as usize;
            format!("{} {}", "#".repeat(level + 2), inline(children(node)))
        }
        "blockquote" => prefix_lines(&blocks(children(node)), "> ", "> "),
        "codeBlock" => {
            let lang = attr(node, "language").and_then(Value::as_str).unwrap_or("");
            let code: String = children(node).iter().filter_map(|t| t.get("text")?.as_str()).collect();
            let fence = if code.contains("```") { "~~~~" } else { "```" };
            format!("{fence}{lang}\n{code}\n{fence}")
        }
        "bulletList" => list(node, |_| "- ".into()),
        "orderedList" => {
            let start = attr(node, "start").and_then(Value::as_u64).unwrap_or(1);
            list(node, |i| format!("{}. ", start + i as u64))
        }
        "taskList" => list(node, |_| String::new()),
        "horizontalRule" => "---".into(),
        _ => blocks(children(node)),
    }
}

/// Each item's blocks, the first line after the marker, the rest indented under it.
fn list(node: &Value, marker: impl Fn(usize) -> String) -> String {
    children(node)
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut m = marker(i);
            if kind(item) == "taskItem" {
                let checked = attr(item, "checked").and_then(Value::as_bool).unwrap_or(false);
                m = format!("- [{}] ", if checked { 'x' } else { ' ' });
            }
            let indent = " ".repeat(m.chars().count());
            prefix_lines(&blocks(children(item)), &m, &indent)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefix_lines(text: &str, first: &str, rest: &str) -> String {
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let p = if i == 0 { first } else { rest };
        if line.is_empty() { out.push_str(p.trim_end()) } else { out.push_str(p); out.push_str(line) }
    }
    out
}

#[derive(Clone, PartialEq)]
enum Mark {
    Link(String),
    Bold,
    Italic,
    Strike,
    Code,
}

impl Mark {
    fn open(&self) -> &'static str {
        match self {
            Mark::Link(_) => "[",
            Mark::Bold => "**",
            Mark::Italic => "_",
            Mark::Strike => "~~",
            Mark::Code => "`",
        }
    }

    fn close(&self) -> String {
        match self {
            Mark::Link(href) => format!("]({})", href.replace(' ', "%20").replace(')', "%29")),
            m => m.open().into(),
        }
    }
}

fn marks_of(node: &Value) -> Vec<Mark> {
    let mut out = Vec::new();
    for m in node.get("marks").and_then(Value::as_array).into_iter().flatten() {
        match kind(m) {
            "link" => {
                let href = attr(m, "href").and_then(Value::as_str).unwrap_or("").to_owned();
                out.push(Mark::Link(href));
            }
            "bold" => out.push(Mark::Bold),
            "italic" => out.push(Mark::Italic),
            "strike" => out.push(Mark::Strike),
            "code" => out.push(Mark::Code),
            _ => {} // textStyle (colour), highlight, underline: no Markdown equivalent
        }
    }
    // Fixed nesting order keeps adjacent runs from reopening the same marks.
    let rank = |m: &Mark| match m {
        Mark::Link(_) => 0,
        Mark::Bold => 1,
        Mark::Italic => 2,
        Mark::Strike => 3,
        Mark::Code => 4,
    };
    out.sort_by_key(rank);
    out
}

/// Inline content with marks opened and closed as a stack, so a bold run that
/// spans an italic word stays one `**…**`. Whitespace at the edges of a run is
/// moved outside the markers (`** a**` is not emphasis in Markdown).
fn inline(nodes: &[Value]) -> String {
    let mut out = String::new();
    let mut open: Vec<Mark> = Vec::new();
    let mut pending_ws = String::new();
    for node in nodes {
        let (marks, text) = match kind(node) {
            "text" => (marks_of(node), node.get("text").and_then(Value::as_str).unwrap_or("")),
            "hardBreak" => {
                // Backslash break (CommonMark), so trailing spaces can be trimmed safely.
                pending_ws.clear();
                out.push_str("\\\n");
                continue;
            }
            _ => continue,
        };
        let is_code = marks.contains(&Mark::Code);
        let core = if is_code { text } else { text.trim() };
        if core.is_empty() {
            pending_ws.push_str(text);
            continue;
        }
        let lead = if is_code { "" } else { &text[..text.len() - text.trim_start().len()] };
        let trail = if is_code { "" } else { &text[text.trim_end().len()..] };

        let keep = open.iter().zip(&marks).take_while(|(a, b)| a == b).count();
        while open.len() > keep {
            out.push_str(&open.pop().unwrap().close());
        }
        out.push_str(&pending_ws);
        out.push_str(lead);
        pending_ws = trail.to_owned();
        for m in &marks[keep..] {
            out.push_str(m.open());
            open.push(m.clone());
        }
        if is_code { out.push_str(core) } else { escape_into(&mut out, core) }
    }
    while let Some(m) = open.pop() {
        out.push_str(&m.close());
    }
    out.push_str(&pending_ws);
    out.split('\n').map(str::trim_end).collect::<Vec<_>>().join("\n")
}

/// Escapes characters that would otherwise start Markdown formatting. Lone
/// underscores inside words (snake_case, #a_b) are left alone.
fn escape_into(out: &mut String, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let word = |j: Option<&char>| j.is_some_and(|c| c.is_alphanumeric());
        match c {
            '\\' | '*' | '`' | '[' | ']' | '<' | '~' => out.push('\\'),
            '_' if !(word(i.checked_sub(1).and_then(|j| chars.get(j))) && word(chars.get(i + 1))) => {
                out.push('\\')
            }
            _ => {}
        }
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(t: &str) -> Value {
        json!({ "type": "text", "text": t })
    }

    fn marked(t: &str, marks: Value) -> Value {
        json!({ "type": "text", "text": t, "marks": marks })
    }

    fn p(content: Value) -> Value {
        json!({ "type": "paragraph", "content": content })
    }

    fn doc(content: Value) -> Value {
        json!({ "type": "doc", "content": content })
    }

    #[test]
    fn converts_inline_marks() {
        let d = doc(json!([p(json!([
            text("a "),
            marked("bold", json!([{ "type": "bold" }])),
            marked(" both", json!([{ "type": "bold" }, { "type": "italic" }])),
            text(" "),
            marked("x*y", json!([{ "type": "code" }])),
            text(" "),
            marked("site", json!([{ "type": "link", "attrs": { "href": "https://e.com/a b" } }])),
            text(" "),
            marked("red", json!([{ "type": "textStyle", "attrs": { "color": "var(--note-red)" } }, { "type": "highlight" }])),
            text(" "),
            marked("gone", json!([{ "type": "strike" }])),
        ]))]));
        assert_eq!(
            to_markdown(&d),
            "a **bold _both_** `x*y` [site](https://e.com/a%20b) red ~~gone~~"
        );
    }

    #[test]
    fn escapes_markdown_in_text() {
        let d = doc(json!([p(json!([text("2*3 [x] snake_case _lead `tick` #tag_x")]))]));
        assert_eq!(to_markdown(&d), "2\\*3 \\[x\\] snake_case \\_lead \\`tick\\` #tag_x");
    }

    #[test]
    fn escapes_block_syntax_at_line_start() {
        let para = |t: &str| to_markdown(&doc(json!([p(json!([text(t)]))])));
        assert_eq!(para("## not a heading"), r"\## not a heading");
        assert_eq!(para("- not a list"), r"\- not a list");
        assert_eq!(para("12. not ordered"), r"12\. not ordered");
        assert_eq!(para("> not quoted"), r"\> not quoted");
        assert_eq!(para("---"), r"\---");
        assert_eq!(para("#tag stays"), "#tag stays");
        assert_eq!(para("C# and 3.5 stay"), "C# and 3.5 stay");
    }

    #[test]
    fn converts_blocks() {
        let d = doc(json!([
            { "type": "heading", "attrs": { "level": 1 }, "content": [text("Title")] },
            { "type": "bulletList", "content": [
                { "type": "listItem", "content": [p(json!([text("one")])),
                    { "type": "orderedList", "attrs": { "start": 3 }, "content": [
                        { "type": "listItem", "content": [p(json!([text("three")]))] },
                        { "type": "listItem", "content": [p(json!([text("four")]))] }
                    ]}
                ]},
                { "type": "listItem", "content": [p(json!([text("two")]))] }
            ]},
            { "type": "taskList", "content": [
                { "type": "taskItem", "attrs": { "checked": true }, "content": [p(json!([text("done")]))] },
                { "type": "taskItem", "attrs": { "checked": false }, "content": [p(json!([text("todo")]))] }
            ]},
            { "type": "blockquote", "content": [p(json!([text("quoted")])), p(json!([text("more")]))] },
            { "type": "codeBlock", "attrs": { "language": "rs" }, "content": [text("let *x = 1;")] },
            p(json!([text("line"), { "type": "hardBreak" }, text("break")])),
        ]));
        assert_eq!(
            to_markdown(&d),
            "### Title\n\n\
             - one\n\n  3. three\n  4. four\n- two\n\n\
             - [x] done\n- [ ] todo\n\n\
             > quoted\n>\n> more\n\n\
             ```rs\nlet *x = 1;\n```\n\n\
             line\\\nbreak"
        );
    }

    #[test]
    fn empty_and_unknown_nodes_do_not_panic() {
        assert_eq!(to_markdown(&json!({})), "");
        assert_eq!(to_markdown(&doc(json!([{ "type": "paragraph" }, { "type": "mystery", "content": [p(json!([text("kept")]))] }]))), "kept");
    }

    #[test]
    fn export_round_trip_every_note_exactly_once() {
        let mut conn = db::open_in_memory().unwrap();
        // Notes a day apart over ~40 days, plus a deleted one.
        let base = 1_760_000_000_000i64;
        let mut ids = Vec::new();
        for i in 0..120i64 {
            // Some notes start with text that looks like Markdown block syntax.
            let prefix = ["", "## ", "- ", "1. ", "> ", "# "][i as usize % 6];
            let t = format!("{prefix}note number {i}");
            let body = db::NoteBody {
                json: doc(json!([p(json!([text(&t)]))])).to_string(),
                html: format!("<p>{t}</p>"),
                text: t,
            };
            ids.push(db::create_note(&mut conn, &body, base + i * 8 * 3_600_000).unwrap().id);
        }
        db::delete_note(&conn, ids[5], base).unwrap();

        let dir = std::env::temp_dir().join(format!("quick-notes-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = export_markdown(&conn, &dir).unwrap();
        assert!((40..=42).contains(&files), "{files} files");

        let mut all = String::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            all += &std::fs::read_to_string(entry.unwrap().path()).unwrap();
        }
        assert_eq!(all.matches("\n## ").count(), 119);
        for i in 0..120 {
            let n = all.matches(&format!("note number {i}\n")).count();
            assert_eq!(n, if i == 5 { 0 } else { 1 }, "note {i}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
