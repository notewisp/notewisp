<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type SettingsView } from "./lib/api";

  let { onClose, onToast, onExport }: { onClose: () => void; onToast: (text: string, error?: boolean) => void; onExport: () => void } = $props();

  const isMac = navigator.userAgent.includes("Mac");
  let settings = $state<SettingsView | null>(null);
  let recording = $state(false);
  let panel: HTMLDivElement;

  onMount(async () => {
    panel.focus();
    try {
      settings = await api.getSettings();
    } catch (e) {
      onToast(errorText(e), true);
    }
  });

  /** "CmdOrCtrl+Shift+Space" → "Ctrl+Shift+Space" (or Cmd on macOS). */
  function display(hotkey: string) {
    return hotkey.replace("CmdOrCtrl", isMac ? "Cmd" : "Ctrl");
  }

  async function save(next: SettingsView) {
    try {
      await api.setSettings({ hotkey: next.hotkey, launch_at_login: next.launch_at_login });
      settings = { ...next, hotkey_error: null };
    } catch (e) {
      onToast(errorText(e), true);
    }
  }

  function record(e: KeyboardEvent) {
    if (!recording || !settings) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      recording = false;
      return;
    }
    if (["Control", "Shift", "Alt", "Meta", "OS"].includes(e.key)) return;
    const mods = [];
    if (e.ctrlKey) mods.push("Ctrl");
    if (e.metaKey) mods.push(isMac ? "Cmd" : "Super");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
    if (!mods.length && !/^F\d+$/.test(key)) {
      onToast("Use at least one modifier (Ctrl, Alt, Shift)", true);
      return;
    }
    recording = false;
    save({ ...settings, hotkey: [...mods, key].join("+") });
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="settings" role="dialog" aria-label="Settings" tabindex="-1" bind:this={panel} onkeydowncapture={record}>
  <div class="settings-head">
    <h2>Settings</h2>
    <button type="button" class="icon-btn" title="Close (Esc)" onclick={onClose}>×</button>
  </div>
  {#if settings}
    <label class="setting">
      <span>Show / hide hotkey</span>
      <button type="button" class="hotkey" class:recording onclick={() => (recording = !recording)}>
        {recording ? "Press keys… (Esc to cancel)" : display(settings.hotkey)}
      </button>
    </label>
    {#if settings.hotkey_error}
      <p class="error-text">This hotkey is unavailable: {settings.hotkey_error}. Pick another.</p>
    {/if}
    <label class="setting">
      <span>Launch at login</span>
      <input
        type="checkbox"
        checked={settings.launch_at_login}
        onchange={(e) => settings && save({ ...settings, launch_at_login: e.currentTarget.checked })}
      />
    </label>
    <div class="setting">
      <span>Export all notes to Markdown</span>
      <button type="button" onclick={onExport}>Export (Ctrl+E)</button>
    </div>
    <p class="muted small">
      Notes are stored locally in one SQLite file and backed up daily (last 7 kept). Light and dark mode follow the system.
    </p>
  {/if}
</div>
