<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import type { Editor, JSONContent } from "@tiptap/core";
  import { createEditor, type SuggestState } from "./lib/editor";
  import type { NoteBody } from "./lib/api";
  import Toolbar from "./Toolbar.svelte";
  import EmojiPicker from "./EmojiPicker.svelte";

  interface Props {
    content?: JSONContent | null;
    placeholder?: string;
    editing?: boolean;
    tags: () => [string, number][];
    onSubmit: (body: NoteBody, startedAt: number) => void;
    onEscape: () => void;
    onEmptyUp?: () => void;
    onChange?: (editor: Editor) => void;
  }

  let {
    content = null,
    placeholder = "Write a note…",
    editing = false,
    tags,
    onSubmit,
    onEscape,
    onEmptyUp,
    onChange,
  }: Props = $props();

  let host: HTMLDivElement;
  let editor = $state.raw<Editor | null>(null);
  let suggest = $state.raw<SuggestState | null>(null);
  let linkOpen = $state(false);
  let emojiOpen = $state(false);

  onMount(() => {
    editor = createEditor(host, content, placeholder, {
      onSubmit: (body, startedAt) => onSubmit(body, startedAt),
      onEscape: () => {
        if (linkOpen || emojiOpen) {
          linkOpen = emojiOpen = false;
        } else {
          onEscape();
        }
      },
      onEmptyUp: onEmptyUp ? () => onEmptyUp?.() : undefined,
      onLinkRequest: () => (linkOpen = true),
      onChange: (e) => onChange?.(e),
      onSuggest: (s) => (suggest = s),
      tags: () => tags(),
    });
    if (editing) editor.commands.focus("end");
  });

  onDestroy(() => editor?.destroy());

  export function focus() {
    editor?.commands.focus();
  }

  export function clear() {
    editor?.commands.clearContent(true);
  }

  export function setContent(json: JSONContent | null) {
    editor?.commands.setContent(json ?? "", { emitUpdate: true });
    editor?.commands.focus("end");
  }

  function insertEmoji(char: string) {
    editor?.chain().focus().insertContent(char).run();
    emojiOpen = false;
  }
</script>

<div class="composer" class:editing>
  <div class="editor-scroll" bind:this={host}></div>
  <button
    class="icon-btn emoji-btn"
    type="button"
    title="Emoji"
    tabindex="-1"
    onmousedown={(e) => e.preventDefault()}
    onclick={() => (emojiOpen = !emojiOpen)}>☺</button
  >
  {#if emojiOpen}
    <EmojiPicker onPick={insertEmoji} onClose={() => { emojiOpen = false; editor?.commands.focus(); }} />
  {/if}
  {#if editor}
    <Toolbar {editor} bind:linkOpen />
  {/if}
  {#if suggest}
    <ul
      class="suggest popover"
      role="listbox"
      style:left="{suggest.rect?.left ?? 0}px"
      style:bottom="{window.innerHeight - (suggest.rect?.top ?? 0) + 6}px"
    >
      {#each suggest.items as item, i (item.insert)}
        <li
          role="option"
          aria-selected={i === suggest.index}
          class:active={i === suggest.index}
          onmousedown={(e) => {
            e.preventDefault();
            suggest?.pick(i);
          }}
        >
          <span class="label">{item.label}</span><span class="detail">{item.detail}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
