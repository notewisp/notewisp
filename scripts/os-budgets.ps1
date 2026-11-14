# Windows-only OS-level budgets for NoteWisp:
#   - cold start: process launch to popup visible and focused (with -Exe, before the rest)
#   - hotkey to window visible and focused (sent with real key events)
#   - memory while idle (hidden in the tray), summed over the app and its WebView2 processes
# Usage: powershell -File scripts/os-budgets.ps1 [-Exe src-tauri/target/release/notewisp.exe] [-Hotkeys 20]
# Without -Exe, NoteWisp must already be running.
param([string]$Exe = "", [int]$Hotkeys = 20, [int]$ColdStarts = 6)

Add-Type -TypeDefinition @"
using System; using System.Diagnostics; using System.Runtime.InteropServices;
public static class NwBench {
  [DllImport("user32.dll")] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  static void Hotkey() {
    keybd_event(0x12,0,0,UIntPtr.Zero); keybd_event(0x4E,0,0,UIntPtr.Zero);
    keybd_event(0x4E,0,2,UIntPtr.Zero); keybd_event(0x12,0,2,UIntPtr.Zero);
  }
  // Presses the default hotkey (Alt+N) and returns ms until the popup is visible and the foreground window.
  public static double ShowLatency(IntPtr h) {
    var sw = Stopwatch.StartNew();
    Hotkey();
    while (!(IsWindowVisible(h) && GetForegroundWindow() == h)) {
      if (sw.ElapsedMilliseconds > 2000) return -1;
      System.Threading.Thread.SpinWait(200);
    }
    return sw.Elapsed.TotalMilliseconds;
  }
  public static void Toggle() { Hotkey(); }
  // Launches the app and returns ms until the popup is visible and focused (it shows itself when ready).
  public static double ColdStart(string exe) {
    var sw = Stopwatch.StartNew();
    Process.Start(exe);
    while (sw.ElapsedMilliseconds < 10000) {
      var h = FindWindow("Tauri Window", "NoteWisp");
      if (h != IntPtr.Zero && IsWindowVisible(h) && GetForegroundWindow() == h) return sw.Elapsed.TotalMilliseconds;
      System.Threading.Thread.Sleep(1);
    }
    return -1;
  }
}
"@

if ($Exe) {
  Get-Process notewisp -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep 1
  $cold = @()
  for ($i = 0; $i -lt $ColdStarts; $i++) {
    $cold += [math]::Round([NwBench]::ColdStart($Exe), 1)
    Start-Sleep -Milliseconds 1500
    Get-Process notewisp | Stop-Process -Force
    Start-Sleep -Milliseconds 1500
  }
  "cold start -> visible+focused ms: $($cold -join ', ')"
  Start-Process $Exe
  Start-Sleep 3
}

$h = [NwBench]::FindWindow("Tauri Window", "NoteWisp")
if ($h -eq [IntPtr]::Zero) { throw "NoteWisp is not running" }

# Start hidden.
if ([NwBench]::IsWindowVisible($h)) { [NwBench]::Toggle(); Start-Sleep -Milliseconds 400 }
$samples = @()
for ($i = 0; $i -lt $Hotkeys; $i++) {
  $samples += [NwBench]::ShowLatency($h)
  Start-Sleep -Milliseconds 250
  [NwBench]::Toggle()        # hide again
  Start-Sleep -Milliseconds 350
}
$sorted = $samples | Sort-Object
"hotkey -> visible+foreground ms: n=$($sorted.Count) p50=$([math]::Round($sorted[[int]($sorted.Count/2)],1)) max=$([math]::Round($sorted[-1],1))"

# Idle memory: the app plus every WebView2 process it started.
Start-Sleep -Seconds 5
$app = Get-Process notewisp
$ids = @($app.Id)
$all = Get-CimInstance Win32_Process
do {
  $more = $all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object { $_.ProcessId }
  $ids += $more
} while ($more)
$procs = Get-Process -Id $ids
$ws = ($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB
$priv = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
# Private working set is what Task Manager shows as "Memory"; working set double-counts shared pages.
$pws = 0
foreach ($p in $procs) {
  $c = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$($p.Id)" -ErrorAction SilentlyContinue
  if ($c) { $pws += $c.WorkingSetPrivate }
}
"idle memory over $($procs.Count) processes: private working set $([math]::Round($pws/1MB,1)) MB (Task Manager), working set $([math]::Round($ws,1)) MB, private commit $([math]::Round($priv,1)) MB"
$procs | Sort-Object WorkingSet64 -Descending | Select-Object Id, ProcessName, @{n="WS_MB";e={[math]::Round($_.WorkingSet64/1MB,1)}}, @{n="Private_MB";e={[math]::Round($_.PrivateMemorySize64/1MB,1)}} | Format-Table -AutoSize | Out-String
