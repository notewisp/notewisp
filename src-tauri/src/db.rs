//! All database access. The webview never touches SQLite; it goes through
//! Tauri commands that call into this module.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Result, Row, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};

/// Extensions the editor writes `body_json` with. Bump when the set changes so
/// old notes can be migrated (stored in `meta.editor_schema`).
pub const EDITOR_SCHEMA: &str = "1";

/// Soft-deleted notes are purged after this long.
pub const PURGE_AFTER_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Numbered migrations. Index + 1 is the schema version after applying it.
/// Never edit a released migration; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    r#"
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);

    CREATE TABLE notes (
      id          INTEGER PRIMARY KEY,
      body_json   TEXT    NOT NULL,
      body_html   TEXT    NOT NULL,
      body_text   TEXT    NOT NULL,
      created_at  INTEGER NOT NULL,
      updated_at  INTEGER NOT NULL,
      deleted_at  INTEGER
    );
    CREATE INDEX notes_created ON notes(created_at);

    CREATE TABLE note_tags (
      note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
      tag     TEXT    NOT NULL,
      PRIMARY KEY (note_id, tag)
    );
    CREATE INDEX note_tags_tag ON note_tags(tag);

    CREATE VIRTUAL TABLE notes_fts USING fts5(
      body_text, content='notes', content_rowid='id', tokenize='unicode61'
    );

    CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
      INSERT INTO notes_fts(rowid, body_text) VALUES (new.id, new.body_text);
    END;
    CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
      INSERT INTO notes_fts(notes_fts, rowid, body_text) VALUES ('delete', old.id, old.body_text);
    END;
    CREATE TRIGGER notes_au AFTER UPDATE OF body_text ON notes BEGIN
      INSERT INTO notes_fts(notes_fts, rowid, body_text) VALUES ('delete', old.id, old.body_text);
      INSERT INTO notes_fts(rowid, body_text) VALUES (new.id, new.body_text);
    END;
    "#,
];

/// A note as sent to the webview: cached HTML for display, plain text for copying.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Note {
    pub id: i64,
    pub html: String,
    pub text: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
}

/// The three forms the editor produces on save. `json` is the source of truth.
#[derive(Debug, Clone, Deserialize)]
pub struct NoteBody {
    pub json: String,
    pub html: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub note: Note,
    /// HTML-escaped excerpt with matches wrapped in `<mark>`.
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub hotkey: String,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { hotkey: "CmdOrCtrl+Shift+Space".into(), launch_at_login: false }
    }
}

/// One note ready for export, with its local day and time already formatted.
pub struct ExportRow {
    pub day: String,
    pub time: String,
    pub json: String,
}

// ---------------------------------------------------------------------------
// Opening and migrations

/// Opens (creating if needed) the database file and brings it up to date.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // WAL makes NORMAL safe against corruption; it only risks the last commit on power loss.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // The backup thread uses its own connection; wait briefly instead of failing on a lock.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    init(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<usize> {
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
        [],
        |r| r.get(0),
    )?;
    if !has_meta {
        return Ok(0);
    }
    Ok(get_meta(conn, "schema_version")?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current = schema_version(conn)?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        set_meta(&tx, "schema_version", &(i + 1).to_string())?;
        tx.commit()?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO meta (key, value) VALUES ('editor_schema', ?1)",
        [EDITOR_SCHEMA],
    )?;
    Ok(())
}

fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)", [key, value])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading notes

const NOTE_COLS: &str = "n.id, n.body_html, n.body_text, n.created_at, n.updated_at,
    (SELECT group_concat(tag, ' ') FROM note_tags WHERE note_id = n.id)";

fn note_from_row(r: &Row) -> Result<Note> {
    let tags: Option<String> = r.get(5)?;
    let mut tags: Vec<String> =
        tags.map(|t| t.split(' ').map(str::to_owned).collect()).unwrap_or_default();
    tags.sort();
    Ok(Note {
        id: r.get(0)?,
        html: r.get(1)?,
        text: r.get(2)?,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
        tags,
    })
}

pub fn get_note(conn: &Connection, id: i64) -> Result<Note> {
    conn.query_row(
        &format!("SELECT {NOTE_COLS} FROM notes n WHERE n.id = ?1 AND n.deleted_at IS NULL"),
        [id],
        note_from_row,
    )
}

pub fn get_note_json(conn: &Connection, id: i64) -> Result<String> {
    conn.query_row(
        "SELECT body_json FROM notes WHERE id = ?1 AND deleted_at IS NULL",
        [id],
        |r| r.get(0),
    )
}

/// A page of live notes, newest first. `before` / `after` are note ids used as
/// keyset cursors on (created_at, id); `tag` restricts to notes with that tag.
pub fn list_notes(
    conn: &Connection,
    before: Option<i64>,
    after: Option<i64>,
    limit: u32,
    tag: Option<&str>,
) -> Result<Vec<Note>> {
    let mut sql = format!("SELECT {NOTE_COLS} FROM notes n WHERE n.deleted_at IS NULL");
    let mut args: Vec<Value> = Vec::new();
    if let Some(tag) = tag {
        sql += " AND EXISTS (SELECT 1 FROM note_tags t WHERE t.note_id = n.id AND t.tag = ?)";
        args.push(tag.to_lowercase().into());
    }
    if let Some(id) = before {
        sql += " AND (n.created_at, n.id) < (SELECT created_at, id FROM notes WHERE id = ?)";
        args.push(id.into());
    }
    if let Some(id) = after {
        sql += " AND (n.created_at, n.id) > (SELECT created_at, id FROM notes WHERE id = ?)";
        args.push(id.into());
    }
    // Paging forwards walks oldest-first so LIMIT keeps the notes nearest the cursor.
    let forwards = after.is_some() && before.is_none();
    sql += if forwards {
        " ORDER BY n.created_at ASC, n.id ASC LIMIT ?"
    } else {
        " ORDER BY n.created_at DESC, n.id DESC LIMIT ?"
    };
    args.push((limit as i64).into());

    let mut stmt = conn.prepare_cached(&sql)?;
    let mut notes = stmt
        .query_map(params_from_iter(args), note_from_row)?
        .collect::<Result<Vec<_>>>()?;
    if forwards {
        notes.reverse();
    }
    Ok(notes)
}

/// The note `id` with up to `radius` notes on each side, newest first.
pub fn notes_around(conn: &Connection, id: i64, radius: u32, tag: Option<&str>) -> Result<Vec<Note>> {
    let center = get_note(conn, id)?;
    let mut notes = list_notes(conn, None, Some(id), radius, tag)?;
    notes.push(center);
    notes.extend(list_notes(conn, Some(id), None, radius, tag)?);
    Ok(notes)
}

pub fn list_tags(conn: &Connection) -> Result<Vec<(String, u32)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag, count(*) AS c FROM note_tags t JOIN notes n ON n.id = t.note_id
         WHERE n.deleted_at IS NULL GROUP BY t.tag ORDER BY c DESC, t.tag",
    )?;
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

// ---------------------------------------------------------------------------
// Writing notes

/// Extracts `#tags` from plain text: lowercase, without '#', de-duplicated.
/// A '#' only starts a tag at the start of a word, so `C#` or `issue#4` don't count.
pub fn parse_tags(text: &str) -> Vec<String> {
    let is_tag_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut tags: Vec<String> = Vec::new();
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let at_word_start = prev.is_none_or(|p| !(p.is_alphanumeric() || p == '_' || p == '&'));
        if c == '#' && at_word_start {
            let start = i + 1;
            let mut end = start;
            while let Some(&(j, d)) = chars.peek() {
                if !is_tag_char(d) {
                    break;
                }
                end = j + d.len_utf8();
                prev = Some(d);
                chars.next();
            }
            if end > start {
                let tag = text[start..end].to_lowercase();
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                continue;
            }
        }
        prev = Some(c);
    }
    tags
}

fn write_tags(conn: &Connection, id: i64, text: &str) -> Result<()> {
    conn.execute("DELETE FROM note_tags WHERE note_id = ?1", [id])?;
    let mut stmt = conn.prepare_cached("INSERT INTO note_tags (note_id, tag) VALUES (?1, ?2)")?;
    for tag in parse_tags(text) {
        stmt.execute(params![id, tag])?;
    }
    Ok(())
}

fn require_text(body: &NoteBody) -> Result<()> {
    if body.text.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName("note is empty".into()));
    }
    Ok(())
}

pub fn create_note(conn: &mut Connection, body: &NoteBody, now: i64) -> Result<Note> {
    require_text(body)?;
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO notes (body_json, body_html, body_text, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![body.json, body.html, body.text, now],
    )?;
    let id = tx.last_insert_rowid();
    write_tags(&tx, id, &body.text)?;
    tx.commit()?;
    get_note(conn, id)
}

pub fn update_note(conn: &mut Connection, id: i64, body: &NoteBody, now: i64) -> Result<Note> {
    require_text(body)?;
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE notes SET body_json = ?2, body_html = ?3, body_text = ?4, updated_at = ?5
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, body.json, body.html, body.text, now],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    write_tags(&tx, id, &body.text)?;
    tx.commit()?;
    get_note(conn, id)
}

pub fn delete_note(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE notes SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    Ok(())
}

pub fn restore_note(conn: &Connection, id: i64) -> Result<Note> {
    conn.execute("UPDATE notes SET deleted_at = NULL WHERE id = ?1", [id])?;
    get_note(conn, id)
}

/// Permanently removes notes deleted more than 30 days ago. Returns how many.
pub fn purge_deleted(conn: &Connection, now: i64) -> Result<usize> {
    conn.execute(
        "DELETE FROM notes WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
        [now - PURGE_AFTER_MS],
    )
}

// ---------------------------------------------------------------------------
// Search

/// Builds an FTS5 query from user input. Every word is quoted (so `"`, `*`,
/// `(` or `-` are just text), words are ANDed, and the last word is a prefix
/// match unless the input ends in whitespace. `#tag` words are returned
/// separately as tag filters.
pub fn parse_query(input: &str) -> (Option<String>, Vec<String>) {
    let mut words = Vec::new();
    let mut tags = Vec::new();
    for word in input.split_whitespace() {
        if word.starts_with('#') {
            tags.extend(parse_tags(word));
        } else if word.chars().any(char::is_alphanumeric) {
            words.push(format!("\"{}\"", word.replace('"', "\"\"")));
        }
    }
    let prefix_last = !input.ends_with(char::is_whitespace);
    if prefix_last && let Some(last) = words.last_mut() {
        last.push('*');
    }
    let fts = if words.is_empty() { None } else { Some(words.join(" ")) };
    (fts, tags)
}

const MARK_OPEN: char = '\u{E000}';
const MARK_CLOSE: char = '\u{E001}';

/// Escapes text for HTML, collapses whitespace, and turns the private-use
/// markers from `snippet()` into `<mark>` tags.
fn snippet_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    let mut last_space = false;
    for c in raw.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
            continue;
        }
        last_space = false;
        match c {
            MARK_OPEN => out.push_str("<mark>"),
            MARK_CLOSE => out.push_str("</mark>"),
            _ => push_escaped(&mut out, c),
        }
    }
    out.trim().to_owned()
}

pub fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#39;"),
        _ => out.push(c),
    }
}

/// Live notes matching `query`, newest first.
pub fn search(conn: &Connection, query: &str, limit: u32) -> Result<Vec<SearchHit>> {
    let (fts, tags) = parse_query(query);
    let mut args: Vec<Value> = Vec::new();
    let mut tag_sql = String::new();
    for tag in &tags {
        tag_sql += " AND EXISTS (SELECT 1 FROM note_tags t WHERE t.note_id = n.id AND t.tag = ?)";
        args.push(tag.clone().into());
    }
    let sql = match &fts {
        // Ids grow with creation time, so walking the index by rowid descending
        // gives newest first and lets LIMIT stop early on broad prefixes.
        Some(fts) => {
            args.insert(0, fts.clone().into());
            format!(
                "SELECT {NOTE_COLS}, snippet(notes_fts, 0, '{MARK_OPEN}', '{MARK_CLOSE}', '…', 16)
                 FROM notes_fts JOIN notes n ON n.id = notes_fts.rowid
                 WHERE notes_fts MATCH ? AND n.deleted_at IS NULL{tag_sql}
                 ORDER BY notes_fts.rowid DESC LIMIT ?"
            )
        }
        None if !tags.is_empty() => format!(
            "SELECT {NOTE_COLS}, substr(n.body_text, 1, 160) FROM notes n
             WHERE n.deleted_at IS NULL{tag_sql}
             ORDER BY n.created_at DESC, n.id DESC LIMIT ?"
        ),
        None => return Ok(Vec::new()),
    };
    args.push((limit as i64).into());
    let mut stmt = conn.prepare_cached(&sql)?;
    stmt.query_map(params_from_iter(args), |r| {
        let snippet: String = r.get(6)?;
        Ok(SearchHit { note: note_from_row(r)?, snippet: snippet_html(&snippet) })
    })?
    .collect()
}

// ---------------------------------------------------------------------------
// Settings and draft

pub fn get_settings(conn: &Connection) -> Result<Settings> {
    Ok(get_meta(conn, "settings")?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn set_settings(conn: &Connection, settings: &Settings) -> Result<()> {
    let json = serde_json::to_string(settings).expect("settings serialize");
    set_meta(conn, "settings", &json)
}

pub fn get_draft(conn: &Connection) -> Result<Option<String>> {
    get_meta(conn, "draft")
}

pub fn set_draft(conn: &Connection, json: Option<&str>) -> Result<()> {
    match json {
        Some(json) => set_meta(conn, "draft", json),
        None => conn.execute("DELETE FROM meta WHERE key = 'draft'", []).map(|_| ()),
    }
}

// ---------------------------------------------------------------------------
// Export and backup

/// Every live note oldest first, with local day (`2026-10-09`) and time (`14:32`).
/// SQLite's `localtime` uses the OS time zone rules, including DST.
pub fn export_rows(conn: &Connection) -> Result<Vec<ExportRow>> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', created_at / 1000, 'unixepoch', 'localtime'),
                strftime('%H:%M', created_at / 1000, 'unixepoch', 'localtime'),
                body_json
         FROM notes WHERE deleted_at IS NULL ORDER BY created_at, id",
    )?;
    stmt.query_map([], |r| Ok(ExportRow { day: r.get(0)?, time: r.get(1)?, json: r.get(2)? }))?
        .collect()
}

/// Local date and time as `YYYY-MM-DD` and `HHMMSS`.
pub fn local_now(conn: &Connection) -> Result<(String, String)> {
    conn.query_row(
        "SELECT date('now', 'localtime'), strftime('%H%M%S', 'now', 'localtime')",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

/// Writes a consistent copy of the database to `path` (which must not exist).
pub fn backup_to(conn: &Connection, path: &Path) -> Result<()> {
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Test data

/// Inserts `count` random notes spread over the past, oldest first. Dev only.
pub fn seed_notes(conn: &mut Connection, count: u32, now: i64) -> Result<()> {
    const WORDS: &[&str] = &[
        "idea", "meeting", "call", "buy", "milk", "fix", "bug", "deploy", "review", "draft",
        "email", "budget", "design", "launch", "release", "server", "notes", "garden", "book",
        "flight", "dentist", "invoice", "refactor", "search", "timeline", "coffee", "lunch",
        "remember", "check", "write", "read", "plan", "sprint", "café", "größe", "naïve", "über",
        "rocket", "keyboard", "window", "popup", "quick", "slow", "later", "tomorrow", "today",
    ];
    const TAGS: &[&str] = &["todo", "idea", "work", "home", "read", "bug", "q4"];
    let mut rng = now as u64 | 1;
    let mut next = move |n: usize| {
        // xorshift64: plenty for test data, no dependency.
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        (rng % n as u64) as usize
    };

    // 1 to 13 minutes apart (gaps never exceed 13 min), so the last note is before `now`.
    let mut t = now - count as i64 * 13 * 60_000;
    let tx = conn.transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO notes (body_json, body_html, body_text, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
        )?;
        let mut tag_stmt = tx.prepare("INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?1, ?2)")?;
        for _ in 0..count {
            t += 60_000 + next(12 * 60_000) as i64;
            let paragraphs = 1 + (next(10) == 0) as usize + (next(20) == 0) as usize;
            let mut texts = Vec::new();
            for _ in 0..paragraphs {
                let len = 3 + next(25);
                let mut words: Vec<String> = (0..len).map(|_| WORDS[next(WORDS.len())].to_owned()).collect();
                if next(4) == 0 {
                    words.push(format!("#{}", TAGS[next(TAGS.len())]));
                }
                texts.push(words.join(" "));
            }
            let json = serde_json::json!({
                "type": "doc",
                "content": texts.iter().map(|p| serde_json::json!({
                    "type": "paragraph", "content": [{ "type": "text", "text": p }]
                })).collect::<Vec<_>>()
            });
            let html: String = texts
                .iter()
                .map(|p| {
                    let mut s = String::from("<p>");
                    p.chars().for_each(|c| push_escaped(&mut s, c));
                    s + "</p>"
                })
                .collect();
            let text = texts.join("\n\n");
            insert.execute(params![json.to_string(), html, text, t])?;
            let id = tx.last_insert_rowid();
            for tag in parse_tags(&text) {
                tag_stmt.execute(params![id, tag])?;
            }
        }
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(text: &str) -> NoteBody {
        NoteBody { json: format!("{{\"text\":{text:?}}}"), html: format!("<p>{text}</p>"), text: text.into() }
    }

    fn insert_raw(conn: &Connection, text: &str) -> i64 {
        conn.execute(
            "INSERT INTO notes (body_json, body_html, body_text, created_at, updated_at)
             VALUES ('{}', '', ?1, 0, 0)",
            [text],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn fts_ids(conn: &Connection, q: &str) -> Vec<i64> {
        let mut stmt = conn
            .prepare("SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([q], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    }

    fn ids(notes: &[Note]) -> Vec<i64> {
        notes.iter().map(|n| n.id).collect()
    }

    fn hit_ids(hits: &[SearchHit]) -> Vec<i64> {
        hits.iter().map(|h| h.note.id).collect()
    }

    // --- schema ---------------------------------------------------------------

    #[test]
    fn migrates_to_latest_and_records_meta() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len());
        assert_eq!(get_meta(&conn, "editor_schema").unwrap().as_deref(), Some(EDITOR_SCHEMA));
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = open_in_memory().unwrap();
        insert_raw(&conn, "keep me");
        migrate(&mut conn).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn fts_follows_insert_update_and_delete() {
        let conn = open_in_memory().unwrap();
        let a = insert_raw(&conn, "buy oat milk");
        let b = insert_raw(&conn, "call the dentist");
        assert_eq!(fts_ids(&conn, "milk"), vec![a]);

        conn.execute("UPDATE notes SET body_text = 'buy oat cream' WHERE id = ?1", [a]).unwrap();
        assert!(fts_ids(&conn, "milk").is_empty());
        assert_eq!(fts_ids(&conn, "cream"), vec![a]);

        conn.execute("DELETE FROM notes WHERE id = ?1", [b]).unwrap();
        assert!(fts_ids(&conn, "dentist").is_empty());
    }

    #[test]
    fn fts_tokenizes_unicode() {
        let conn = open_in_memory().unwrap();
        let id = insert_raw(&conn, "Café Größe naïve");
        assert_eq!(fts_ids(&conn, "größe"), vec![id]);
        assert_eq!(fts_ids(&conn, "caf*"), vec![id]);
    }

    #[test]
    fn file_database_uses_wal_and_reopens() {
        let dir = std::env::temp_dir().join(format!("quick-notes-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.db");
        {
            let conn = open(&path).unwrap();
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
            assert_eq!(mode, "wal");
            insert_raw(&conn, "persisted");
            backup_to(&conn, &dir.join("backup.db")).unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(fts_ids(&conn, "persisted").len(), 1);
        let backup = open(&dir.join("backup.db")).unwrap();
        assert_eq!(fts_ids(&backup, "persisted").len(), 1);
        drop((conn, backup));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // --- tags -----------------------------------------------------------------

    #[test]
    fn parses_tags() {
        assert_eq!(parse_tags("#Todo buy #milk, then #todo"), vec!["todo", "milk"]);
        assert_eq!(parse_tags("#größe #a_b-c #2026"), vec!["größe", "a_b-c", "2026"]);
        assert!(parse_tags("C# and issue#4 and # alone and &#39;").is_empty());
        assert_eq!(parse_tags("(#paren) end#no"), vec!["paren"]);
    }

    #[test]
    fn deleting_a_note_cascades_to_tags() {
        let mut conn = open_in_memory().unwrap();
        let note = create_note(&mut conn, &body("#todo thing"), 1).unwrap();
        conn.execute("DELETE FROM notes WHERE id = ?1", [note.id]).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM note_tags", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    // --- writes ---------------------------------------------------------------

    #[test]
    fn create_returns_note_with_tags() {
        let mut conn = open_in_memory().unwrap();
        let note = create_note(&mut conn, &body("ship it #Work #q4"), 1000).unwrap();
        assert_eq!(note.tags, vec!["q4", "work"]);
        assert_eq!((note.created_at, note.updated_at), (1000, 1000));
        assert_eq!(note.html, "<p>ship it #Work #q4</p>");
        assert_eq!(get_note_json(&conn, note.id).unwrap(), body("ship it #Work #q4").json);
    }

    #[test]
    fn rejects_empty_notes() {
        let mut conn = open_in_memory().unwrap();
        assert!(create_note(&mut conn, &body("  \n\t "), 1).is_err());
        let id = create_note(&mut conn, &body("x"), 1).unwrap().id;
        assert!(update_note(&mut conn, id, &body(" "), 2).is_err());
    }

    #[test]
    fn update_rewrites_tags_and_keeps_created_at() {
        let mut conn = open_in_memory().unwrap();
        let id = create_note(&mut conn, &body("#a #b"), 1).unwrap().id;
        let note = update_note(&mut conn, id, &body("#b #c"), 5).unwrap();
        assert_eq!(note.tags, vec!["b", "c"]);
        assert_eq!((note.created_at, note.updated_at), (1, 5));
        assert!(ids(&list_notes(&conn, None, None, 10, Some("a")).unwrap()).is_empty());
        assert_eq!(ids(&list_notes(&conn, None, None, 10, Some("c")).unwrap()), vec![id]);
        assert_eq!(list_tags(&conn).unwrap(), vec![("b".into(), 1), ("c".into(), 1)]);
    }

    #[test]
    fn soft_delete_hides_everywhere_and_restore_brings_back() {
        let mut conn = open_in_memory().unwrap();
        let id = create_note(&mut conn, &body("secret #x"), 1).unwrap().id;
        delete_note(&conn, id, 2).unwrap();
        assert!(list_notes(&conn, None, None, 10, None).unwrap().is_empty());
        assert!(search(&conn, "secret", 10).unwrap().is_empty());
        assert!(search(&conn, "#x", 10).unwrap().is_empty());
        assert!(list_tags(&conn).unwrap().is_empty());
        assert!(get_note(&conn, id).is_err());
        assert!(update_note(&mut conn, id, &body("y"), 3).is_err());

        let note = restore_note(&conn, id).unwrap();
        assert_eq!(note.tags, vec!["x"]);
        assert_eq!(hit_ids(&search(&conn, "secret", 10).unwrap()), vec![id]);
    }

    #[test]
    fn purges_only_old_deleted_notes() {
        let mut conn = open_in_memory().unwrap();
        let old = create_note(&mut conn, &body("old"), 1).unwrap().id;
        let recent = create_note(&mut conn, &body("recent"), 1).unwrap().id;
        create_note(&mut conn, &body("live"), 1).unwrap();
        let now = PURGE_AFTER_MS + 1000;
        delete_note(&conn, old, 10).unwrap();
        delete_note(&conn, recent, now - 10).unwrap();
        assert_eq!(purge_deleted(&conn, now).unwrap(), 1);
        assert!(restore_note(&conn, old).is_err());
        assert!(restore_note(&conn, recent).is_ok());
    }

    // --- paging ---------------------------------------------------------------

    fn make_notes(conn: &mut Connection, n: i64) -> Vec<i64> {
        (0..n).map(|i| create_note(conn, &body(&format!("note {i}")), 1000 + i).unwrap().id).collect()
    }

    #[test]
    fn pages_newest_first_with_cursors() {
        let mut conn = open_in_memory().unwrap();
        let all = make_notes(&mut conn, 10);
        let page1 = ids(&list_notes(&conn, None, None, 4, None).unwrap());
        assert_eq!(page1, vec![all[9], all[8], all[7], all[6]]);
        let page2 = ids(&list_notes(&conn, Some(all[6]), None, 4, None).unwrap());
        assert_eq!(page2, vec![all[5], all[4], all[3], all[2]]);
        let newer = ids(&list_notes(&conn, None, Some(all[2]), 3, None).unwrap());
        assert_eq!(newer, vec![all[5], all[4], all[3]]);
    }

    #[test]
    fn same_timestamp_notes_page_without_gaps() {
        let mut conn = open_in_memory().unwrap();
        let all: Vec<i64> = (0..5).map(|_| create_note(&mut conn, &body("same"), 7).unwrap().id).collect();
        let first = ids(&list_notes(&conn, None, None, 2, None).unwrap());
        let rest = ids(&list_notes(&conn, Some(first[1]), None, 10, None).unwrap());
        let mut seen = [first, rest].concat();
        seen.reverse();
        assert_eq!(seen, all);
    }

    #[test]
    fn notes_around_centres_on_the_note() {
        let mut conn = open_in_memory().unwrap();
        let all = make_notes(&mut conn, 10);
        let around = ids(&notes_around(&conn, all[5], 2, None).unwrap());
        assert_eq!(around, vec![all[7], all[6], all[5], all[4], all[3]]);
        let edge = ids(&notes_around(&conn, all[0], 2, None).unwrap());
        assert_eq!(edge, vec![all[2], all[1], all[0]]);
    }

    // --- search ---------------------------------------------------------------

    #[test]
    fn builds_safe_fts_queries() {
        assert_eq!(parse_query("buy mil"), (Some("\"buy\" \"mil\"*".into()), vec![]));
        assert_eq!(parse_query("buy "), (Some("\"buy\"".into()), vec![]));
        assert_eq!(parse_query("say \"hi\""), (Some("\"say\" \"\"\"hi\"\"\"*".into()), vec![]));
        assert_eq!(parse_query("#Work plan"), (Some("\"plan\"*".into()), vec!["work".into()]));
        assert_eq!(parse_query("( - * \""), (None, vec![]));
    }

    #[test]
    fn hostile_queries_never_error() {
        let mut conn = open_in_memory().unwrap();
        create_note(&mut conn, &body("a (b) c-d \"e\" f*g NEAR AND OR NOT"), 1).unwrap();
        for q in [
            "\"", "*", "(", ")", "-", "-x", "a\"b", "\"unterminated", "(b", "c-d", "f*g", "NEAR(",
            "AND", "OR x", "NOT", "^", ":", "col:x", "#", "##", "# x", "{", "+", "'", "\\", "%_",
        ] {
            search(&conn, q, 10).unwrap_or_else(|e| panic!("query {q:?} failed: {e}"));
        }
        assert_eq!(search(&conn, "c-d", 10).unwrap().len(), 1);
        assert_eq!(search(&conn, "AND", 10).unwrap().len(), 1);
    }

    #[test]
    fn search_prefix_matches_last_word_and_orders_newest_first() {
        let mut conn = open_in_memory().unwrap();
        let a = create_note(&mut conn, &body("deploy the server"), 1).unwrap().id;
        let b = create_note(&mut conn, &body("deployment notes"), 2).unwrap().id;
        create_note(&mut conn, &body("unrelated"), 3).unwrap();
        assert_eq!(hit_ids(&search(&conn, "depl", 10).unwrap()), vec![b, a]);
        assert_eq!(hit_ids(&search(&conn, "deploy ", 10).unwrap()), vec![a]);
        assert_eq!(hit_ids(&search(&conn, "the serv", 10).unwrap()), vec![a]);
    }

    #[test]
    fn search_combines_tags_and_text() {
        let mut conn = open_in_memory().unwrap();
        let a = create_note(&mut conn, &body("fix login #bug"), 1).unwrap().id;
        let b = create_note(&mut conn, &body("fix typo #docs"), 2).unwrap().id;
        let c = create_note(&mut conn, &body("crash on start #bug"), 3).unwrap().id;
        assert_eq!(hit_ids(&search(&conn, "#bug", 10).unwrap()), vec![c, a]);
        assert_eq!(hit_ids(&search(&conn, "#bug fix", 10).unwrap()), vec![a]);
        assert_eq!(hit_ids(&search(&conn, "fix", 10).unwrap()), vec![b, a]);
    }

    #[test]
    fn snippets_are_escaped_and_highlighted() {
        let mut conn = open_in_memory().unwrap();
        create_note(&mut conn, &body("<b>bold</b> & milk\n\nmore"), 1).unwrap();
        let hit = &search(&conn, "milk", 10).unwrap()[0];
        assert_eq!(hit.snippet, "&lt;b&gt;bold&lt;/b&gt; &amp; <mark>milk</mark> more");
    }

    // --- settings, draft, export, seed ------------------------------------------

    #[test]
    fn settings_and_draft_round_trip() {
        let conn = open_in_memory().unwrap();
        assert_eq!(get_settings(&conn).unwrap(), Settings::default());
        let s = Settings { hotkey: "Alt+N".into(), launch_at_login: true };
        set_settings(&conn, &s).unwrap();
        assert_eq!(get_settings(&conn).unwrap(), s);

        assert_eq!(get_draft(&conn).unwrap(), None);
        set_draft(&conn, Some("{\"type\":\"doc\"}")).unwrap();
        assert_eq!(get_draft(&conn).unwrap().as_deref(), Some("{\"type\":\"doc\"}"));
        set_draft(&conn, None).unwrap();
        assert_eq!(get_draft(&conn).unwrap(), None);
    }

    #[test]
    fn export_rows_are_oldest_first_and_live_only() {
        let mut conn = open_in_memory().unwrap();
        let a = create_note(&mut conn, &body("first"), 1_760_000_000_000).unwrap().id;
        let b = create_note(&mut conn, &body("second"), 1_760_000_060_000).unwrap().id;
        delete_note(&conn, a, 1).unwrap();
        let rows = export_rows(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].json, get_note_json(&conn, b).unwrap());
        assert_eq!(rows[0].day.len(), 10);
        assert_eq!(rows[0].time.len(), 5);
    }

    /// Budget check: `cargo test --release -- --ignored --nocapture budgets`
    #[test]
    #[ignore]
    fn budgets_on_100k_notes() {
        use std::time::Instant;
        let dir = std::env::temp_dir().join(format!("quick-notes-bench-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut conn = open(&dir.join("notes.db")).unwrap();
        let t = Instant::now();
        seed_notes(&mut conn, 100_000, 1_760_000_000_000).unwrap();
        println!("seed 100k: {:?}", t.elapsed());

        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        let mut worst: f64 = 0.0;
        // Every prefix of a few queries, as if typed one keystroke at a time.
        for q in ["m", "mi", "mil", "milk", "milk c", "milk co", "milk coffee", "#todo", "#todo d", "#todo depl",
                  "\"", "(", "a*b", "-x", "größ", "zzzz", "e", "t"] {
            let t = Instant::now();
            let hits = search(&conn, q, 50).unwrap();
            let el = ms(t);
            worst = worst.max(el);
            println!("search {q:<14} {:>3} hits {el:>7.2} ms", hits.len());
        }
        println!("worst search: {worst:.2} ms");
        assert!(worst < 50.0);

        let t = Instant::now();
        let page = list_notes(&conn, None, None, 200, None).unwrap();
        println!("first page: {:.2} ms", ms(t));
        let t = Instant::now();
        let deep = list_notes(&conn, Some(page[199].id - 90_000), None, 200, None).unwrap();
        println!("deep page: {:.2} ms ({} notes)", ms(t), deep.len());
        let t = Instant::now();
        list_notes(&conn, None, None, 200, Some("todo")).unwrap();
        println!("tag page: {:.2} ms", ms(t));
        let t = Instant::now();
        notes_around(&conn, 50_000, 100, None).unwrap();
        println!("notes_around: {:.2} ms", ms(t));
        let t = Instant::now();
        let mut c = conn;
        create_note(&mut c, &body("new note #todo"), 1_760_000_000_001).unwrap();
        println!("create: {:.2} ms", ms(t));
        drop(c);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn seeds_notes_in_time_order() {
        let mut conn = open_in_memory().unwrap();
        seed_notes(&mut conn, 500, 1_760_000_000_000).unwrap();
        let notes = list_notes(&conn, None, None, 1000, None).unwrap();
        assert_eq!(notes.len(), 500);
        assert!(notes.windows(2).all(|w| w[0].created_at > w[1].created_at && w[0].id > w[1].id));
        assert!(notes[0].created_at < 1_760_000_000_000);
        assert!(!list_tags(&conn).unwrap().is_empty());
        assert!(!search(&conn, "milk", 5).unwrap().is_empty());
    }
}
