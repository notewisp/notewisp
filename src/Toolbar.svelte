<script lang="ts">
  // Floating format bar shown over a text selection, plus the link editor and
  // colour palette. All HTML popovers in the same window: a native dialog would
  // take focus and hide the popup.
  import { tick } from "svelte";
  import type { Editor } from "@tiptap/core";
  import { PALETTE } from "./lib/editor";

  let { editor, linkOpen = $bindable(false) }: { editor: Editor; linkOpen?: boolean } = $props();

  let visible = $state(false);
  let x = $state(0);
  let y = $state(0);
  let below = $state(false);
  let paletteOpen = $state(false);
  let href = $state("");
  let linkInput = $state<HTMLInputElement>();
  let bar = $state<HTMLDivElement>();
  let active = $state({ bold: false, italic: false, heading: false, link: false, highlight: false });

  function update() {
    const { selection } = editor.state;
    const show = linkOpen || (!selection.empty && editor.isFocused && !editor.isActive("codeBlock"));
    if (!show) {
      visible = false;
      paletteOpen = false;
      return;
    }
    const view = editor.view;
    const start = view.coordsAtPos(selection.from);
    const end = view.coordsAtPos(selection.to);
    const sameLine = Math.abs(start.top - end.top) < 4;
    x = sameLine ? (start.left + end.right) / 2 : start.left;
    below = start.top < 64;
    y = below ? end.bottom + 8 : start.top - 8;
    active = {
      bold: editor.isActive("bold"),
      italic: editor.isActive("italic"),
      heading: editor.isActive("heading"),
      link: editor.isActive("link"),
      highlight: editor.isActive("highlight"),
    };
    visible = true;
  }

  $effect(() => {
    const onBlur = () => setTimeout(update, 0);
    editor.on("selectionUpdate", update);
    editor.on("focus", update);
    editor.on("blur", onBlur);
    return () => {
      editor.off("selectionUpdate", update);
      editor.off("focus", update);
      editor.off("blur", onBlur);
    };
  });

  // Keep the whole bar inside the window (it is centred on x).
  // Measured after the DOM updates, since the link input changes the width.
  let left = $state(0);
  $effect(() => {
    void [x, linkOpen, visible];
    if (!bar) return;
    const half = bar.offsetWidth / 2;
    left = Math.max(half + 4, Math.min(window.innerWidth - half - 4, x));
  });

  // Opening the link editor (button or Ctrl+K) prefills and focuses it.
  $effect(() => {
    if (!linkOpen) return;
    href = editor.getAttributes("link").href ?? "";
    update();
    tick().then(() => linkInput?.select());
  });

  function cycleHeading() {
    const level = [1, 2, 3].find((l) => editor.isActive("heading", { level: l }));
    const chain = editor.chain().focus();
    if (level === 3) chain.setParagraph().run();
    else chain.setHeading({ level: ((level ?? 0) + 1) as 1 | 2 | 3 }).run();
  }

  function applyLink() {
    const url = href.trim();
    const full = !url || /^[a-z][a-z0-9+.-]*:/i.test(url) ? url : `https://${url}`;
    if (!full) {
      editor.chain().focus().extendMarkRange("link").unsetLink().run();
    } else if (editor.state.selection.empty && !editor.isActive("link")) {
      editor
        .chain()
        .focus()
        .insertContent([{ type: "text", text: url, marks: [{ type: "link", attrs: { href: full } }] }, { type: "text", text: " " }])
        .run();
    } else {
      editor.chain().focus().extendMarkRange("link").setLink({ href: full }).run();
    }
    linkOpen = false;
  }

  function closeLink() {
    linkOpen = false;
    editor.commands.focus();
  }

  function setColor(c: string | null) {
    const chain = editor.chain().focus();
    if (c) chain.setColor(`var(--note-${c})`).run();
    else chain.unsetColor().run();
    paletteOpen = false;
  }

  const keep = (e: MouseEvent) => e.preventDefault();
</script>

{#if visible}
  <div class="toolbar popover" bind:this={bar} class:below style:left="{left}px" style:top="{y}px" role="toolbar" tabindex="-1" onmousedown={keep}>
    {#if linkOpen}
      <input
        class="link-input"
        bind:this={linkInput}
        bind:value={href}
        placeholder="Paste or type a link"
        onmousedown={(e) => e.stopPropagation()}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            applyLink();
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            closeLink();
          }
        }}
        onblur={() => (linkOpen = false)}
      />
    {:else}
      <button type="button" class:on={active.bold} title="Bold (Ctrl+B)" onclick={() => editor.chain().focus().toggleBold().run()}><b>B</b></button>
      <button type="button" class:on={active.italic} title="Italic (Ctrl+I)" onclick={() => editor.chain().focus().toggleItalic().run()}><i>I</i></button>
      <button type="button" class:on={active.heading} title="Heading (Ctrl+Alt+1–3)" onclick={cycleHeading}>H</button>
      <button type="button" class:on={active.link} title="Link (Ctrl+K)" onclick={() => (linkOpen = true)}>🔗</button>
      <button type="button" title="Text colour" onclick={() => (paletteOpen = !paletteOpen)}><span class="swatch-a">A</span></button>
      <button type="button" class:on={active.highlight} title="Highlight" onclick={() => editor.chain().focus().toggleHighlight().run()}><mark>ab</mark></button>
      {#if paletteOpen}
        <div class="palette">
          {#each PALETTE as c (c)}
            <button type="button" class="swatch" title={c} style:background="var(--note-{c})" onclick={() => setColor(c)} aria-label={c}></button>
          {/each}
          <button type="button" class="swatch none" title="Default colour" onclick={() => setColor(null)}>×</button>
        </div>
      {/if}
    {/if}
  </div>
{/if}
