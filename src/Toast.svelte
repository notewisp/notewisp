<script lang="ts" module>
  export interface ToastData {
    text: string;
    error?: boolean;
    action?: { label: string; run: () => void };
  }
</script>

<script lang="ts">
  let { toast, onDismiss }: { toast: ToastData | null; onDismiss: () => void } = $props();
</script>

{#if toast}
  <div class="toast" class:error={toast.error} role="status" aria-live="polite">
    <span>{toast.text}</span>
    {#if toast.action}
      <button
        type="button"
        onmousedown={(e) => e.preventDefault()}
        onclick={() => {
          toast?.action?.run();
          onDismiss();
        }}>{toast.action.label}</button
      >
    {/if}
  </div>
{/if}
