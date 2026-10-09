<script lang="ts">
  // In-app emoji picker. The OS emoji panel would take focus and hide the popup.
  import { onMount } from "svelte";
  import { loadEmojis, matchEmojis, type EmojiItem } from "./lib/emoji";

  let { onPick, onClose }: { onPick: (emoji: string) => void; onClose: () => void } = $props();

  let all = $state.raw<EmojiItem[]>([]);
  let query = $state("");
  let input: HTMLInputElement;

  const groups = $derived.by(() => {
    if (query.trim()) return [{ name: "Results", items: matchEmojis(all, query, 120) }];
    const map = new Map<string, EmojiItem[]>();
    for (const e of all) {
      const g = e.group || "other";
      if (!map.has(g)) map.set(g, []);
      map.get(g)!.push(e);
    }
    return [...map].map(([name, items]) => ({ name, items }));
  });

  onMount(() => {
    input.focus();
    loadEmojis().then((e) => (all = e));
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const first = groups[0]?.items[0];
      if (first) onPick(first.emoji);
    }
  }
</script>

<div class="emoji-picker popover" role="dialog" aria-label="Emoji">
  <input bind:this={input} bind:value={query} placeholder="Search emoji" onkeydown={onKey} />
  <div class="emoji-groups">
    {#each groups as g (g.name)}
      <div class="emoji-group-name">{g.name}</div>
      <div class="emoji-grid">
        {#each g.items as e (e.name)}
          <button type="button" title=":{e.shortcodes[0] ?? e.name}:" onmousedown={(ev) => ev.preventDefault()} onclick={() => onPick(e.emoji)}>{e.emoji}</button>
        {/each}
      </div>
    {:else}
      <div class="muted pad">Loading…</div>
    {/each}
  </div>
</div>
