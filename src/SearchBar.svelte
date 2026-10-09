<script lang="ts">
  import DOMPurify from "dompurify";
  import { api, errorText, type SearchHit } from "./lib/api";
  import { dayLabel, timeLabel } from "./lib/time";
  import { perf } from "./lib/perf";

  interface Props {
    tagFilter: string | null;
    onPick: (hit: SearchHit) => void;
    onClose: () => void;
    onClearTag: () => void;
    onError: (msg: string) => void;
    onSettings: () => void;
  }

  let { tagFilter, onPick, onClose, onClearTag, onError, onSettings }: Props = $props();

  let input: HTMLInputElement;
  let list = $state<HTMLDivElement>();
  let query = $state("");
  let hits = $state.raw<SearchHit[]>([]);
  let index = $state(0);
  let seq = 0;

  export const active = () => query.trim() !== "";

  export function focus() {
    input.focus();
    input.select();
  }

  export function clear() {
    query = "";
    hits = [];
    seq++;
  }

  async function run() {
    const my = ++seq;
    const q = query;
    if (!q.trim()) {
      hits = [];
      return;
    }
    const t0 = performance.now();
    try {
      const r = await api.search(q, 50);
      if (my !== seq) return;
      hits = r;
      index = 0;
      perf.search.push(performance.now() - t0);
    } catch (e) {
      if (my === seq) onError(errorText(e));
    }
  }

  function pick(hit: SearchHit | undefined) {
    if (!hit) return;
    clear();
    onPick(hit);
  }

  function move(step: number) {
    if (!hits.length) return;
    index = (index + step + hits.length) % hits.length;
    list?.querySelector(`[data-i="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }

  function onKey(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "Enter":
        e.preventDefault();
        pick(hits[index]);
        break;
      case "Escape":
        e.preventDefault();
        clear();
        onClose();
        break;
    }
  }

  const snippet = (html: string) => DOMPurify.sanitize(html, { ALLOWED_TAGS: ["mark"], ALLOWED_ATTR: [] });
</script>

<header class="searchbar">
  <span class="search-icon" aria-hidden="true">⌕</span>
  <input
    bind:this={input}
    bind:value={query}
    oninput={run}
    onkeydown={onKey}
    placeholder="Search notes (Ctrl+F) · #tag to filter"
    spellcheck="false"
    aria-label="Search notes"
  />
  {#if tagFilter}
    <button type="button" class="filter-chip" title="Clear filter (Esc)" onclick={onClearTag}>#{tagFilter} ×</button>
  {/if}
  <button type="button" class="icon-btn" title="Settings" tabindex="-1" onclick={onSettings}>⚙</button>
</header>

{#if query.trim()}
  <div class="results" bind:this={list} role="listbox">
    {#each hits as hit, i (hit.id)}
      <button
        type="button"
        class="result"
        class:active={i === index}
        data-i={i}
        role="option"
        aria-selected={i === index}
        onmousedown={(e) => e.preventDefault()}
        onmousemove={() => (index = i)}
        onclick={() => pick(hit)}
      >
        <span class="result-when muted">{dayLabel(hit.created_at)} · {timeLabel(hit.created_at)}</span>
        <span class="result-snippet">{@html snippet(hit.snippet)}</span>
      </button>
    {:else}
      <div class="muted pad">No matching notes</div>
    {/each}
  </div>
{/if}
