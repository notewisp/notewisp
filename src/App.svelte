<script lang="ts">
  import { onMount, tick } from "svelte";
  import type { Editor, JSONContent } from "@tiptap/core";
  import { listen } from "@tauri-apps/api/event";
  import { api, errorText, PAGE, type Note, type NoteBody, type SearchHit } from "./lib/api";
  import { isBlank } from "./lib/editor";
  import { parseTags } from "./lib/tags";
  import { perf } from "./lib/perf";
  import Timeline from "./Timeline.svelte";
  import NoteItem from "./NoteItem.svelte";
  import Composer from "./Composer.svelte";
  import SearchBar from "./SearchBar.svelte";
  import Toast, { type ToastData } from "./Toast.svelte";
  import Settings from "./Settings.svelte";

  /** A note plus a stable list key, so an optimistic note keeps its row when its real id arrives. */
  type Row = Note & { key: number };
  const toRow = (n: Note): Row => ({ ...n, key: n.id });

  let notes = $state.raw<Row[]>([]);
  let hasOlder = $state(false);
  let hasNewer = $state(false);
  let tagFilter = $state<string | null>(null);
  let selectedKey = $state<number | null>(null);
  let highlightKey = $state<number | null>(null);
  let editing = $state.raw<{ key: number; id: number; content: JSONContent } | null>(null);
  let tags = $state.raw<[string, number][]>([]);
  let settingsOpen = $state(false);
  let draft = $state.raw<JSONContent | null>(null);
  let ready = $state(false);
  let toast = $state.raw<ToastData | null>(null);

  let timeline = $state<Timeline<Row>>();
  let composer = $state<Composer>();
  let searchBar = $state<SearchBar>();

  let loadingOlder = false;
  let loadingNewer = false;
  let tempSeq = 0;
  let toastTimer: ReturnType<typeof setTimeout>;
  let draftTimer: ReturnType<typeof setTimeout>;
  let highlightTimer: ReturnType<typeof setTimeout>;

  // --- Toasts -----------------------------------------------------------------

  function showToast(text: string, opts: { error?: boolean; action?: ToastData["action"] } = {}) {
    clearTimeout(toastTimer);
    toast = { text, ...opts };
    toastTimer = setTimeout(() => (toast = null), 5000);
  }

  const fail = (e: unknown) => showToast(errorText(e), { error: true });

  // --- Loading ------------------------------------------------------------------

  async function loadLatest() {
    const page = await api.listNotes(null, null, PAGE, tagFilter);
    notes = page.reverse().map(toRow);
    hasOlder = page.length === PAGE;
    hasNewer = false;
    await tick();
    timeline?.scrollToBottom();
  }

  async function loadOlder() {
    const first = notes.find((n) => n.id > 0);
    if (loadingOlder || !hasOlder || !first) return;
    loadingOlder = true;
    try {
      const page = await api.listNotes(first.id, null, PAGE, tagFilter);
      hasOlder = page.length === PAGE;
      notes = [...page.reverse().map(toRow), ...notes];
    } catch (e) {
      fail(e);
    } finally {
      loadingOlder = false;
    }
  }

  async function loadNewer() {
    const last = notes.findLast((n) => n.id > 0);
    if (loadingNewer || !hasNewer || !last) return;
    loadingNewer = true;
    try {
      const page = await api.listNotes(null, last.id, PAGE, tagFilter);
      hasNewer = page.length === PAGE;
      notes = [...notes, ...page.reverse().map(toRow)];
    } catch (e) {
      fail(e);
    } finally {
      loadingNewer = false;
    }
  }

  async function refreshTags() {
    try {
      tags = await api.listTags();
    } catch {
      // Autocomplete only; not worth a toast.
    }
  }

  // --- Capture ------------------------------------------------------------------

  /** Shows the note in the same frame as Enter, then saves it. */
  async function submit(body: NoteBody, startedAt: number) {
    clearTimeout(draftTimer);
    const key = -++tempSeq;
    const now = Date.now();
    const temp: Row = { id: key, key, html: body.html, text: body.text, created_at: now, updated_at: now, tags: parseTags(body.text) };
    const json = body.json;
    composer?.clear();

    if (hasNewer || (tagFilter && !temp.tags.includes(tagFilter))) {
      tagFilter = null;
      await loadLatest().catch(fail);
    }
    selectedKey = null;
    notes = [...notes, temp];
    await tick();
    timeline?.scrollToBottom();
    requestAnimationFrame(() => perf.enterToVisible.push(performance.now() - startedAt));

    try {
      const saved = await api.createNote(body);
      notes = notes.map((n) => (n.key === key ? { ...saved, key } : n));
      if (saved.tags.length) refreshTags();
    } catch (e) {
      notes = notes.filter((n) => n.key !== key);
      composer?.setContent(JSON.parse(json));
      fail(e);
    }
  }

  function onDraftChange(editor: Editor) {
    clearTimeout(draftTimer);
    const json = isBlank(editor) ? null : JSON.stringify(editor.getJSON());
    draftTimer = setTimeout(() => api.saveDraft(json).catch(fail), 300);
  }

  // --- Edit and delete ------------------------------------------------------------

  async function startEdit(note: Row) {
    if (note.id < 0) return;
    try {
      const content = JSON.parse(await api.getNoteJson(note.id));
      editing = { key: note.key, id: note.id, content };
      selectedKey = note.key;
    } catch (e) {
      fail(e);
    }
  }

  async function editLast() {
    if (hasNewer || tagFilter) {
      tagFilter = null;
      await loadLatest().catch(fail);
    }
    const last = notes.findLast((n) => n.id > 0);
    if (last) {
      await startEdit(last);
      await tick();
      timeline?.revealKey(last.key);
    }
  }

  function cancelEdit() {
    editing = null;
    selectedKey = null;
    composer?.focus();
  }

  async function saveEdit(body: NoteBody) {
    const current = editing;
    if (!current) return;
    if (body.text.trim() === "") {
      editing = null;
      const note = notes.find((n) => n.key === current.key);
      if (note) remove(note);
      composer?.focus();
      return;
    }
    try {
      const saved = await api.updateNote(current.id, body);
      editing = null;
      selectedKey = null;
      notes = tagFilter && !saved.tags.includes(tagFilter)
        ? notes.filter((n) => n.key !== current.key)
        : notes.map((n) => (n.key === current.key ? { ...saved, key: n.key } : n));
      refreshTags();
      composer?.focus();
    } catch (e) {
      fail(e);
    }
  }

  async function remove(note: Row) {
    if (note.id < 0) return;
    const index = notes.findIndex((n) => n.key === note.key);
    notes = notes.filter((n) => n.key !== note.key);
    if (editing?.key === note.key) editing = null;
    try {
      await api.deleteNote(note.id);
      refreshTags();
      showToast("Note deleted", { action: { label: "Undo", run: () => restore(note, index) } });
    } catch (e) {
      notes = [...notes.slice(0, index), note, ...notes.slice(index)];
      fail(e);
    }
  }

  async function restore(note: Row, index: number) {
    try {
      const back = await api.restoreNote(note.id);
      if (!notes.some((n) => n.key === note.key)) {
        const i = Math.min(index, notes.length);
        notes = [...notes.slice(0, i), { ...back, key: note.key }, ...notes.slice(i)];
      }
      refreshTags();
    } catch (e) {
      fail(e);
    }
  }

  async function copy(note: Row) {
    try {
      await navigator.clipboard.writeText(note.text);
      showToast("Copied");
    } catch (e) {
      fail(e);
    }
  }

  // --- Tags, search and jumping ---------------------------------------------------

  async function setTag(tag: string | null) {
    tagFilter = tag;
    editing = null;
    await loadLatest().catch(fail);
  }

  async function jumpTo(hit: SearchHit) {
    try {
      tagFilter = null;
      editing = null;
      const radius = 100;
      const around = await api.notesAround(hit.id, radius, null);
      const i = around.findIndex((n) => n.id === hit.id);
      hasNewer = i === radius; // a full page above it (newest first) means there's more
      hasOlder = around.length - i - 1 === radius;
      notes = around.reverse().map(toRow);
      selectedKey = highlightKey = hit.id;
      await tick();
      timeline?.scrollToKey(hit.id);
      clearTimeout(highlightTimer);
      highlightTimer = setTimeout(() => (highlightKey = null), 1600);
      document.querySelector<HTMLElement>(`[data-note-id="${hit.id}"]`)?.focus({ preventScroll: true });
    } catch (e) {
      fail(e);
    }
  }

  // --- Window, keys, maintenance --------------------------------------------------

  async function exportAll() {
    try {
      const files = await api.exportMarkdown();
      showToast(`Exported to ${files} Markdown file${files === 1 ? "" : "s"}`);
    } catch (e) {
      fail(e);
    }
  }

  async function seed() {
    showToast("Seeding 100,000 notes…");
    const t0 = performance.now();
    try {
      await api.seedNotes(100_000);
      await loadLatest();
      refreshTags();
      showToast(`Seeded 100,000 notes in ${((performance.now() - t0) / 1000).toFixed(1)} s`);
    } catch (e) {
      fail(e);
    }
  }

  /** Esc: close the innermost thing; with nothing open, hide the popup. */
  function escape() {
    if (settingsOpen) settingsOpen = false;
    else if (editing) cancelEdit();
    else if (searchBar?.active()) searchBar.clear();
    else if (tagFilter) setTag(null);
    else if (hasNewer) loadLatest().catch(fail);
    else {
      selectedKey = null;
      api.hidePopup();
      return;
    }
    composer?.focus();
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey && !e.shiftKey) {
      const action = { f: () => searchBar?.focus(), e: exportAll, q: () => api.quit(), ",": () => (settingsOpen = true) }[
        e.key.toLowerCase()
      ];
      if (action) {
        e.preventDefault();
        action();
        return;
      }
    }
    if (import.meta.env.DEV && mod && e.altKey && e.shiftKey && e.code === "KeyS") {
      e.preventDefault();
      seed();
      return;
    }
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      escape();
    }
  }

  /** The popup was shown: the editor gets focus, and we record the hotkey latency. */
  function onWindowFocus() {
    if (!ready || settingsOpen || editing) return;
    if (!searchBar?.active()) composer?.focus();
    api.perfSinceHotkey().then((ms) => ms != null && perf.hotkeyToFocus.push(ms));
  }

  onMount(() => {
    const unlisten = listen("open-settings", () => (settingsOpen = true));
    (async () => {
      try {
        const [page, draftJson] = await Promise.all([api.listNotes(null, null, PAGE, null), api.getDraft()]);
        notes = page.reverse().map(toRow);
        hasOlder = page.length === PAGE;
        draft = draftJson ? JSON.parse(draftJson) : null;
      } catch (e) {
        fail(e);
      }
      ready = true;
      await tick();
      timeline?.scrollToBottom();
      composer?.focus();
      perf.coldStart = await api.appReady();
      composer?.focus();
      refreshTags();
    })();
    return () => unlisten.then((f) => f());
  });
</script>

<svelte:window onkeydown={onKey} onfocus={onWindowFocus} />

<div class="popup">
  <SearchBar
    bind:this={searchBar}
    {tagFilter}
    onPick={jumpTo}
    onClose={() => composer?.focus()}
    onClearTag={() => setTag(null)}
    onError={(m) => showToast(m, { error: true })}
    onSettings={() => (settingsOpen = !settingsOpen)}
  />
  <main class="main">
    <Timeline bind:this={timeline} items={notes} {hasOlder} {hasNewer} onNeedOlder={loadOlder} onNeedNewer={loadNewer}>
      {#snippet row(note: Row, i: number)}
        <NoteItem
          {note}
          prev={notes[i - 1]}
          selected={selectedKey === note.key}
          highlighted={highlightKey === note.key}
          editContent={editing?.key === note.key ? editing.content : null}
          tags={() => tags}
          onSelect={() => (selectedKey = note.key)}
          onEdit={() => startEdit(note)}
          onDelete={() => remove(note)}
          onCopy={() => copy(note)}
          onTag={(t) => setTag(t)}
          onLink={(href) => api.openLink(href).catch(fail)}
          onEditSubmit={saveEdit}
          onEditCancel={cancelEdit}
        />
      {/snippet}
      {#snippet empty()}
        {#if ready}
          <div class="empty muted">
            {tagFilter ? `No notes tagged #${tagFilter}` : "No notes yet. Type below and press Enter."}
          </div>
        {/if}
      {/snippet}
    </Timeline>
  </main>
  {#if ready}
    <Composer
      bind:this={composer}
      content={draft}
      placeholder="Write a note… Enter to save, Shift+Enter for a new line"
      tags={() => tags}
      onSubmit={submit}
      onEscape={escape}
      onEmptyUp={editLast}
      onChange={onDraftChange}
    />
  {/if}
  {#if settingsOpen}
    <Settings onClose={() => { settingsOpen = false; composer?.focus(); }} onToast={(t, error) => showToast(t, { error })} onExport={exportAll} />
  {/if}
  <Toast {toast} onDismiss={() => (toast = null)} />
</div>
