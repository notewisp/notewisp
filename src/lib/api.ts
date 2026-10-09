// Typed wrappers for the Rust commands. The webview never touches the database.
import { invoke } from "@tauri-apps/api/core";

export interface Note {
  id: number;
  html: string;
  text: string;
  created_at: number;
  updated_at: number;
  tags: string[];
}

export interface SearchHit extends Note {
  snippet: string;
}

export interface NoteBody {
  json: string;
  html: string;
  text: string;
}

export interface Settings {
  hotkey: string;
  launch_at_login: boolean;
}

export interface SettingsView extends Settings {
  hotkey_error: string | null;
}

export const PAGE = 200;

export const api = {
  listNotes: (before: number | null, after: number | null, limit: number, tag: string | null) =>
    invoke<Note[]>("list_notes", { before, after, limit, tag }),
  notesAround: (id: number, radius: number, tag: string | null) =>
    invoke<Note[]>("notes_around", { id, radius, tag }),
  getNoteJson: (id: number) => invoke<string>("get_note_json", { id }),
  createNote: (body: NoteBody) => invoke<Note>("create_note", { body }),
  updateNote: (id: number, body: NoteBody) => invoke<Note>("update_note", { id, body }),
  deleteNote: (id: number) => invoke<void>("delete_note", { id }),
  restoreNote: (id: number) => invoke<Note>("restore_note", { id }),
  search: (query: string, limit: number) => invoke<SearchHit[]>("search", { query, limit }),
  listTags: () => invoke<[string, number][]>("list_tags"),
  getDraft: () => invoke<string | null>("get_draft"),
  saveDraft: (json: string | null) => invoke<void>("save_draft", { json }),
  getSettings: () => invoke<SettingsView>("get_settings"),
  setSettings: (s: Settings) => invoke<void>("set_settings", { s }),
  appReady: () => invoke<number>("app_ready"),
  hidePopup: () => invoke<void>("hide_popup"),
  quit: () => invoke<void>("quit_app"),
  perfSinceHotkey: () => invoke<number | null>("perf_since_hotkey"),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  exportMarkdown: () => invoke<number>("export_markdown"),
  seedNotes: (count: number) => invoke<void>("seed_notes", { count }),
};

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
