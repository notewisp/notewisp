<script lang="ts" generics="T extends { key: number }">
  // Virtualised, chat-style list: oldest at the top, opens at the bottom.
  // Only rows near the viewport are in the DOM. Row heights are measured as
  // rows render; until then an estimate is used, and the scroll position is
  // re-anchored to the row at the top of the view so nothing jumps when
  // estimates are corrected or older pages are prepended.
  import { onDestroy, untrack, type Snippet } from "svelte";

  interface Props {
    items: T[];
    hasOlder: boolean;
    hasNewer: boolean;
    onNeedOlder: () => void;
    onNeedNewer: () => void;
    row: Snippet<[T, number]>;
    empty?: Snippet;
  }

  let { items, hasOlder, hasNewer, onNeedOlder, onNeedNewer, row, empty }: Props = $props();

  const ESTIMATE = 52;
  const OVERSCAN = 600;
  const EDGE = 1500;

  let viewport: HTMLDivElement;
  let scrollTop = $state(0);
  let height = $state(0);
  let measured = $state(0);
  const heights = new Map<number, number>();

  let stick = true;
  let anchor: { key: number; delta: number } | null = null;

  const layout = $derived.by(() => {
    void measured;
    const n = items.length;
    const offsets = new Float64Array(n + 1);
    const index = new Map<number, number>();
    for (let i = 0; i < n; i++) {
      const k = items[i].key;
      index.set(k, i);
      offsets[i + 1] = offsets[i] + (heights.get(k) ?? ESTIMATE);
    }
    return { offsets, index, total: offsets[n] };
  });

  // Short timelines sit at the bottom, like a chat.
  const base = $derived(Math.max(0, height - layout.total));

  /** Index of the row containing content-y `y`. */
  function rowAt(y: number): number {
    const o = layout.offsets;
    let lo = 0;
    let hi = items.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (o[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return Math.max(0, lo);
  }

  const range = $derived.by(() => {
    if (!items.length) return [0, 0];
    const top = scrollTop - base;
    return [rowAt(top - OVERSCAN), Math.min(items.length, rowAt(top + height + OVERSCAN) + 1)];
  });

  const visible = $derived(items.slice(range[0], range[1]));

  const ro = new ResizeObserver((entries) => {
    let changed = false;
    for (const entry of entries) {
      const el = entry.target as HTMLElement;
      const key = Number(el.dataset.key);
      const h = entry.borderBoxSize?.[0]?.blockSize ?? el.offsetHeight;
      if (h > 0 && heights.get(key) !== h) {
        heights.set(key, h);
        changed = true;
      }
    }
    if (changed) measured++;
  });
  onDestroy(() => ro.disconnect());

  function measure(node: HTMLElement) {
    ro.observe(node);
    return { destroy: () => ro.unobserve(node) };
  }

  function captureAnchor() {
    const top = viewport.scrollTop - base;
    const i = rowAt(top);
    const item = items[i];
    anchor = item ? { key: item.key, delta: layout.offsets[i] - top } : null;
  }

  function restore() {
    if (!viewport) return;
    if (stick) {
      viewport.scrollTop = viewport.scrollHeight;
    } else if (anchor) {
      const i = layout.index.get(anchor.key);
      if (i !== undefined) viewport.scrollTop = base + layout.offsets[i] - anchor.delta;
    }
    scrollTop = viewport.scrollTop;
    if (layout.total < height + EDGE && hasOlder) onNeedOlder();
  }

  // After every layout change (new items, measured heights), put the view back.
  $effect(() => {
    void layout;
    void base;
    untrack(restore);
  });

  function onScroll() {
    const st = viewport.scrollTop;
    scrollTop = st;
    const fromBottom = viewport.scrollHeight - st - viewport.clientHeight;
    stick = fromBottom < 4 && !hasNewer;
    captureAnchor();
    if (st < EDGE && hasOlder) onNeedOlder();
    if (fromBottom < EDGE && hasNewer) onNeedNewer();
  }

  export function scrollToBottom() {
    stick = true;
    restore();
  }

  export function isAtBottom() {
    return stick;
  }

  /** Brings a row into view, centred, and keeps it there while rows around it are measured. */
  export function scrollToKey(key: number) {
    const i = layout.index.get(key);
    if (i === undefined) return;
    stick = false;
    const h = heights.get(key) ?? ESTIMATE;
    const top = Math.max(0, layout.offsets[i] - (height - h) / 2);
    anchor = { key, delta: layout.offsets[i] - top };
    restore();
  }

  /** Scrolls just enough to show a row (keyboard selection). */
  export function revealKey(key: number) {
    const i = layout.index.get(key);
    if (i === undefined) return;
    const top = base + layout.offsets[i];
    const bottom = base + layout.offsets[i + 1];
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (bottom > viewport.scrollTop + height) viewport.scrollTop = bottom - height;
  }
</script>

<div class="timeline" bind:this={viewport} bind:clientHeight={height} onscroll={onScroll}>
  {#if items.length === 0 && empty}
    {@render empty()}
  {/if}
  <div class="timeline-spacer" style:height="{Math.max(layout.total + base, height)}px">
    {#each visible as item, j (item.key)}
      <div class="timeline-row" data-key={item.key} style:top="{base + layout.offsets[range[0] + j]}px" use:measure>
        {@render row(item, range[0] + j)}
      </div>
    {/each}
  </div>
</div>
