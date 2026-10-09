// The TipTap editor used by the composer and by in-place editing. Only these
// two are ever live editors; timeline notes render from cached HTML.
import { Editor, Extension, type JSONContent, type Range } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { Color, TextStyle } from "@tiptap/extension-text-style";
import Highlight from "@tiptap/extension-highlight";
import { Placeholder } from "@tiptap/extensions";
import { Suggestion } from "@tiptap/suggestion";
import { PluginKey } from "@tiptap/pm/state";
import type { NoteBody } from "./api";
import { loadEmojis, matchEmojis } from "./emoji";

/** Recorded in meta.editor_schema; bump with the Rust constant when extensions change. */
export const EDITOR_SCHEMA = "1";

export const PALETTE = ["red", "orange", "yellow", "green", "teal", "blue", "purple", "pink"] as const;

export interface SuggestItem {
  /** What's shown in the list, e.g. "🚀" + ":rocket:" or "#todo" + "12". */
  label: string;
  detail: string;
  insert: string;
}

export interface SuggestState {
  items: SuggestItem[];
  index: number;
  /** Caret rectangle, for placing the list. */
  rect: DOMRect | null;
  pick: (i: number) => void;
}

export interface EditorHooks {
  onSubmit: (body: NoteBody, startedAt: number) => void;
  /** Esc with nothing else to close. */
  onEscape: () => void;
  /** Up arrow in an empty editor. */
  onEmptyUp?: () => void;
  onLinkRequest: () => void;
  onChange?: (editor: Editor) => void;
  onSuggest: (s: SuggestState | null) => void;
  /** Tags for `#` autocomplete. */
  tags: () => [string, number][];
}

export function bodyOf(editor: Editor): NoteBody {
  return {
    json: JSON.stringify(editor.getJSON()),
    html: editor.getHTML(),
    text: editor.getText({ blockSeparator: "\n\n" }),
  };
}

export function isBlank(editor: Editor): boolean {
  return editor.getText().trim() === "";
}

/** One `Suggestion` plugin driving the shared list UI through `hooks.onSuggest`. */
function suggestion(
  editor: Editor,
  hooks: EditorHooks,
  active: { current: boolean },
  opts: { char: string; key: string; items: (q: string) => Promise<SuggestItem[]> | SuggestItem[] },
) {
  let state: SuggestState | null = null;
  let range: Range | null = null;

  const pick = (i: number) => {
    const item = state?.items[i];
    if (!item || !range) return;
    editor.chain().focus().insertContentAt(range, item.insert + " ").run();
  };

  const publish = (items: SuggestItem[], rect: DOMRect | null, index = 0) => {
    state = items.length ? { items, index: Math.min(index, items.length - 1), rect, pick } : null;
    active.current = state !== null;
    hooks.onSuggest(state);
  };

  return Suggestion<SuggestItem, SuggestItem>({
    editor,
    char: opts.char,
    pluginKey: new PluginKey(opts.key),
    allowedPrefixes: [" ", "(", " "],
    items: ({ query }) => opts.items(query),
    command: ({ editor, range, props }) => {
      editor.chain().focus().insertContentAt(range, props.insert + " ").run();
    },
    render: () => ({
      onStart: (p) => {
        range = p.range;
        publish(p.items, p.clientRect?.() ?? null);
      },
      onUpdate: (p) => {
        range = p.range;
        publish(p.items, p.clientRect?.() ?? null);
      },
      onExit: () => {
        range = null;
        publish([], null);
      },
      onKeyDown: ({ event }) => {
        if (!state) return false;
        const n = state.items.length;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          const step = event.key === "ArrowDown" ? 1 : -1;
          publish(state.items, state.rect, (state.index + step + n) % n);
          return true;
        }
        if (event.key === "Enter" || event.key === "Tab") {
          pick(state.index);
          return true;
        }
        if (event.key === "Escape") {
          publish([], null);
          return true;
        }
        return false;
      },
    }),
  });
}

export function createEditor(element: HTMLElement, content: JSONContent | null, placeholder: string, hooks: EditorHooks) {
  // True while an autocomplete list with items is open; Enter then picks instead of saving.
  const suggesting = { current: false };

  const Suggestions = Extension.create({
    name: "quickNotesSuggestions",
    priority: 1000,
    addProseMirrorPlugins() {
      const editor = this.editor;
      return [
        suggestion(editor, hooks, suggesting, {
          char: ":",
          key: "emojiSuggest",
          items: async (q) => {
            if (q.length < 2) return [];
            const all = await loadEmojis();
            return matchEmojis(all, q, 8).map((e) => ({
              label: e.emoji,
              detail: `:${e.shortcodes[0] ?? e.name}:`,
              insert: e.emoji,
            }));
          },
        }),
        suggestion(editor, hooks, suggesting, {
          char: "#",
          key: "tagSuggest",
          items: (q) => {
            const lower = q.toLowerCase();
            return hooks
              .tags()
              .filter(([t]) => t.startsWith(lower) && t !== lower)
              .slice(0, 8)
              .map(([t, n]) => ({ label: "#" + t, detail: String(n), insert: "#" + t }));
          },
        }),
      ];
    },
  });

  const inBlockWithOwnEnter = (e: Editor) =>
    e.isActive("listItem") || e.isActive("taskItem") || e.isActive("codeBlock");

  const submit = (e: Editor) => {
    const startedAt = performance.now();
    if (!isBlank(e)) hooks.onSubmit(bodyOf(e), startedAt);
    return true;
  };

  const Keys = Extension.create({
    name: "quickNotesKeys",
    // Above the core and list keymaps, below the suggestion plugins.
    priority: 500,
    addKeyboardShortcuts() {
      return {
        Enter: ({ editor }) => {
          if (suggesting.current || inBlockWithOwnEnter(editor)) return false;
          return submit(editor);
        },
        "Mod-Enter": ({ editor }) => (suggesting.current ? false : submit(editor)),
        "Shift-Enter": ({ editor }) => {
          if (inBlockWithOwnEnter(editor)) return false;
          // A new paragraph (not a <br>), so Markdown shortcuts work on the new line.
          return editor.commands.splitBlock();
        },
        ArrowUp: ({ editor }) => {
          if (!hooks.onEmptyUp || !editor.isEmpty) return false;
          hooks.onEmptyUp();
          return true;
        },
        Escape: () => {
          if (suggesting.current) return false;
          hooks.onEscape();
          return true;
        },
        "Mod-k": () => {
          hooks.onLinkRequest();
          return true;
        },
      };
    },
  });

  return new Editor({
    element,
    content: content ?? "",
    autofocus: false,
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        underline: false,
        horizontalRule: false,
        trailingNode: false,
        link: {
          openOnClick: false,
          autolink: true,
          linkOnPaste: true,
          defaultProtocol: "https",
          protocols: ["http", "https", "mailto"],
          HTMLAttributes: { target: null, rel: "noopener noreferrer" },
        },
      }),
      TaskList,
      TaskItem.configure({ nested: true }),
      TextStyle,
      Color,
      Highlight,
      Placeholder.configure({ placeholder }),
      Suggestions,
      Keys,
    ],
    editorProps: {
      attributes: { class: "prose", spellcheck: "true" },
      // Keep structure (bold, links, lists, headings) but not foreign colours, fonts or classes.
      transformPastedHTML: (html) => html.replace(/\s(style|class)="[^"]*"/gi, ""),
    },
    onUpdate: ({ editor }) => hooks.onChange?.(editor),
  });
}
