<script lang="ts">
  import type { JSONContent } from "@tiptap/core";
  import type { Note, NoteBody } from "./lib/api";
  import { renderNote } from "./lib/render";
  import { dayKey, dayLabel, fullLabel, timeLabel } from "./lib/time";
  import Composer from "./Composer.svelte";

  interface Props {
    note: Note;
    prev: Note | undefined;
    selected: boolean;
    highlighted: boolean;
    /** Set while this note is open in the editor. */
    editContent: JSONContent | null;
    tags: () => [string, number][];
    onSelect: () => void;
    onEdit: () => void;
    onDelete: () => void;
    onCopy: () => void;
    onTag: (tag: string) => void;
    onLink: (href: string) => void;
    onEditSubmit: (body: NoteBody) => void;
    onEditCancel: () => void;
  }

  let p: Props = $props();

  const showDay = $derived(!p.prev || dayKey(p.prev.created_at) !== dayKey(p.note.created_at));
  const edited = $derived(p.note.updated_at - p.note.created_at > 1000);
  const pending = $derived(p.note.id < 0);
  const html = $derived(renderNote(p.note));

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    const tag = target.closest<HTMLElement>("button.tag");
    if (tag?.dataset.tag) {
      e.preventDefault();
      p.onTag(tag.dataset.tag);
      return;
    }
    const link = target.closest<HTMLAnchorElement>("a[href]");
    if (link) {
      e.preventDefault();
      p.onLink(link.getAttribute("href")!);
      return;
    }
    p.onSelect();
  }

  function onKey(e: KeyboardEvent) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      p.onDelete();
    } else if (e.key === "Enter") {
      e.preventDefault();
      p.onEdit();
    }
  }
</script>

{#if showDay}
  <div class="day-sep"><span>{dayLabel(p.note.created_at)}</span></div>
{/if}
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="note"
  class:selected={p.selected}
  class:highlighted={p.highlighted}
  class:pending
  class:is-editing={p.editContent !== null}
  data-note-id={p.note.id}
  tabindex="0"
  role="article"
  onclick={onClick}
  ondblclick={() => p.onEdit()}
  onkeydown={onKey}
>
  <time datetime={new Date(p.note.created_at).toISOString()} title={fullLabel(p.note.created_at)}>{timeLabel(p.note.created_at)}</time>
  {#if p.editContent !== null}
    <div class="note-edit">
      <Composer
        content={p.editContent}
        editing
        placeholder="Edit note (empty deletes it)"
        tags={p.tags}
        onSubmit={(body) => p.onEditSubmit(body)}
        onEscape={() => p.onEditCancel()}
      />
      <div class="edit-hint muted">Enter to save · Esc to cancel</div>
    </div>
  {:else}
    <div class="note-body prose">{@html html}</div>
    {#if edited}<span class="edited muted" title="Edited {fullLabel(p.note.updated_at)}">edited</span>{/if}
    <div class="note-actions">
      <button type="button" class="icon-btn" title="Copy as text" tabindex="-1" onclick={(e) => { e.stopPropagation(); p.onCopy(); }}>⧉</button>
      <button type="button" class="icon-btn" title="Edit" tabindex="-1" onclick={(e) => { e.stopPropagation(); p.onEdit(); }}>✎</button>
      <button type="button" class="icon-btn" title="Delete" tabindex="-1" onclick={(e) => { e.stopPropagation(); p.onDelete(); }}>🗑</button>
    </div>
  {/if}
</div>
