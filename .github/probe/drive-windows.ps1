param([string]$Exe, [string]$Out, [string]$ProbeDir)
# Drives the real Airtype on a GitHub-hosted Windows runner. ASCII only: Windows PowerShell 5.1 reads this file.
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force $Out | Out-Null
$Mock = 'http://localhost:8799'
$AppLog = 'C:\Windows\Temp\airtype-probe.log'
$DriverLog = Join-Path $Out 'driver.log'
$Results = Join-Path $Out 'results.txt'
$Utf8 = New-Object Text.UTF8Encoding($false)

function Say($s) {
  $line = '[{0:HH:mm:ss.fff}] {1}' -f (Get-Date), $s
  [IO.File]::AppendAllText($DriverLog, "$line`r`n", $Utf8)
  Write-Host ($line -replace '[^\x20-\x7e]', '?')
}
function Result($s) {
  [IO.File]::AppendAllText($Results, "$s`r`n", $Utf8)
  Say "RESULT $s"
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing, System.Windows.Forms -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class W {
  [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Explicit)] struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public UNION u; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
  delegate bool EnumProc(IntPtr h, IntPtr l);

  [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] p, int size);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint c, uint t);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr h, IntPtr dc);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr dst, int x, int y, int w, int h, IntPtr src, int sx, int sy, uint rop);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, StringBuilder l, uint flags, uint ms, out IntPtr result);
  [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint ms, out IntPtr result);
  [DllImport("user32.dll", SetLastError = true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetUserObjectInformation(IntPtr obj, int index, StringBuilder info, int size, out int needed);

  static bool Extended(ushort vk) { return (vk >= 0x21 && vk <= 0x28) || vk == 0x2D || vk == 0x2E; }

  static INPUT Key(ushort vk, bool up) {
    INPUT i = new INPUT();
    i.type = 1;
    i.u.ki.wVk = vk;
    i.u.ki.wScan = (ushort)MapVirtualKey(vk, 0);
    i.u.ki.dwFlags = (up ? 2u : 0u) | (Extended(vk) ? 1u : 0u);
    return i;
  }

  static uint One(INPUT i) { return SendInput(1, new INPUT[] { i }, Marshal.SizeOf(typeof(INPUT))); }

  // Presses the keys in order, holds, lets go in reverse. Returns how many events the system took.
  public static uint Chord(ushort[] vks) {
    uint sent = 0;
    foreach (ushort vk in vks) { sent += One(Key(vk, false)); Thread.Sleep(25); }
    Thread.Sleep(70);
    for (int n = vks.Length - 1; n >= 0; n--) { sent += One(Key(vks[n], true)); Thread.Sleep(25); }
    return sent;
  }

  public static uint TypeText(string s) {
    uint sent = 0;
    foreach (char c in s) {
      INPUT i = new INPUT();
      i.type = 1;
      i.u.ki.wScan = c;
      i.u.ki.dwFlags = 4;
      sent += One(i);
      i.u.ki.dwFlags = 6;
      sent += One(i);
      Thread.Sleep(2);
    }
    return sent;
  }

  public static string Cls(IntPtr h) { StringBuilder s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static string Title(IntPtr h) { StringBuilder s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static uint Pid(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
  public static string Describe(IntPtr h) {
    if (h == IntPtr.Zero) return "(none)";
    RECT r; GetWindowRect(h, out r);
    return string.Format("hwnd={0} pid={1} class={2} title=[{3}] visible={4} rect={5},{6} {7}x{8}", h, Pid(h), Cls(h), Title(h), IsWindowVisible(h), r.L, r.T, r.R - r.L, r.B - r.T);
  }

  // Top-level windows; pid 0 means every visible titled window on the desktop.
  public static string ListTop(uint pid) {
    StringBuilder o = new StringBuilder();
    EnumWindows(delegate (IntPtr h, IntPtr l) {
      if (pid == 0 ? (IsWindowVisible(h) && Title(h).Length > 0) : Pid(h) == pid) o.AppendLine(Describe(h));
      return true;
    }, IntPtr.Zero);
    return o.ToString();
  }

  public static IntPtr FindTop(uint pid, string cls, string title, bool visibleOnly) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate (IntPtr h, IntPtr l) {
      if (pid != 0 && Pid(h) != pid) return true;
      if (!string.IsNullOrEmpty(cls) && Cls(h) != cls) return true;
      if (!string.IsNullOrEmpty(title) && Title(h).IndexOf(title, StringComparison.Ordinal) < 0) return true;
      if (visibleOnly && !IsWindowVisible(h)) return true;
      found = h;
      return false;
    }, IntPtr.Zero);
    return found;
  }

  public static IntPtr FindExact(uint pid, string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate (IntPtr h, IntPtr l) {
      if (Pid(h) == pid && Title(h) == title && IsWindowVisible(h)) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  public static string Children(IntPtr top) {
    StringBuilder o = new StringBuilder();
    EnumChildWindows(top, delegate (IntPtr h, IntPtr l) { o.AppendLine(Describe(h)); return true; }, IntPtr.Zero);
    return o.ToString();
  }

  public static IntPtr FindEdit(IntPtr top) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(top, delegate (IntPtr h, IntPtr l) {
      string c = Cls(h);
      if (c == "Edit" || c.StartsWith("RichEdit")) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  public static string GetText(IntPtr h) {
    IntPtr n;
    SendMessageTimeout(h, 0x000E, IntPtr.Zero, IntPtr.Zero, 2, 1000, out n);
    StringBuilder s = new StringBuilder(n.ToInt32() + 2);
    IntPtr got;
    SendMessageTimeout(h, 0x000D, new IntPtr(s.Capacity), s, 2, 1000, out got);
    return s.ToString();
  }

  // EM_GETSEL: start in the low word, end in the high word.
  public static string GetSel(IntPtr h) {
    IntPtr r;
    SendMessageTimeout(h, 0x00B0, IntPtr.Zero, IntPtr.Zero, 2, 1000, out r);
    long v = r.ToInt64();
    return string.Format("{0}-{1}", v & 0xFFFF, (v >> 16) & 0xFFFF);
  }

  public static bool Front(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    // Alt lifts the foreground lock; it is only sent when something else is in front.
    keybd_event(0x12, 0, 0, UIntPtr.Zero);
    keybd_event(0x12, 0, 2, UIntPtr.Zero);
    ShowWindow(h, 9);
    uint p;
    uint fg = GetWindowThreadProcessId(GetForegroundWindow(), out p);
    uint me = GetCurrentThreadId();
    if (fg != 0) AttachThreadInput(me, fg, true);
    SetForegroundWindow(h);
    BringWindowToTop(h);
    if (fg != 0) AttachThreadInput(me, fg, false);
    Thread.Sleep(250);
    return GetForegroundWindow() == h;
  }

  public static string InputDesktop() {
    IntPtr d = OpenInputDesktop(0, false, 0x0001);
    if (d == IntPtr.Zero) return "OpenInputDesktop failed: " + Marshal.GetLastWin32Error();
    StringBuilder s = new StringBuilder(256);
    int needed;
    GetUserObjectInformation(d, 2, s, 512, out needed);
    return s.ToString();
  }

  public static void Shot(string path) {
    Rectangle b = System.Windows.Forms.SystemInformation.VirtualScreen;
    using (Bitmap bmp = new Bitmap(b.Width, b.Height, PixelFormat.Format32bppArgb))
    using (Graphics g = Graphics.FromImage(bmp)) {
      IntPtr screen = GetDC(IntPtr.Zero);
      IntPtr dc = g.GetHdc();
      // SRCCOPY | CAPTUREBLT: layered windows, which the panel and the slip are, included.
      BitBlt(dc, 0, 0, b.Width, b.Height, screen, b.Left, b.Top, 0x40CC0020);
      g.ReleaseHdc(dc);
      ReleaseDC(IntPtr.Zero, screen);
      bmp.Save(path, ImageFormat.Png);
    }
  }

  // One window, drawn by itself with its web content (PW_RENDERFULLCONTENT).
  public static bool ShotWindow(IntPtr h, string path) {
    RECT r;
    if (!GetWindowRect(h, out r)) return false;
    int w = r.R - r.L, t = r.B - r.T;
    if (w <= 0 || t <= 0) return false;
    using (Bitmap bmp = new Bitmap(w, t, PixelFormat.Format32bppArgb))
    using (Graphics g = Graphics.FromImage(bmp)) {
      IntPtr dc = g.GetHdc();
      bool ok = PrintWindow(h, dc, 2);
      g.ReleaseHdc(dc);
      bmp.Save(path, ImageFormat.Png);
      return ok;
    }
  }
}
'@

[void][W]::SetProcessDPIAware()

$VK = @{ Ctrl = 0x11; Alt = 0x12; Shift = 0x10; Esc = 0x1B; Space = 0x20; End = 0x23; Home = 0x24; Right = 0x27; Del = 0x2E; A = 0x41; T = 0x54 }
function Keys([string[]]$names) {
  [uint16[]]$codes = $names | ForEach-Object { [uint16]$VK[$_] }
  return [W]::Chord($codes)
}
function Shot($name) {
  try { [W]::Shot((Join-Path $Out "$name.png")) } catch { Say "shot $name failed: $_" }
}
function Rest($pathAndQuery) { return Invoke-RestMethod -Uri "$Mock$pathAndQuery" -TimeoutSec 5 }
function Norm($s) { if ($null -eq $s) { return '' }; return ($s -replace "`r`n", "`n" -replace "`r", "`n").Trim() }
function AppLogTail($since) {
  if (-not (Test-Path $AppLog)) { return '(no app log)' }
  $all = [IO.File]::ReadAllLines($AppLog, $Utf8)
  return ($all | Select-Object -Skip $since) -join ' | '
}
function AppLogCount { if (Test-Path $AppLog) { return ([IO.File]::ReadAllLines($AppLog, $Utf8)).Count }; return 0 }

# Clipboard: plain text plus an HTML format, to see whether more than text comes back.
$HtmlFormat = [Windows.Forms.DataFormats]::Html
function SetClip($text) {
  for ($i = 0; $i -lt 10; $i++) {
    try {
      $d = New-Object Windows.Forms.DataObject
      $d.SetData([Windows.Forms.DataFormats]::UnicodeText, $text)
      $d.SetData($HtmlFormat, "<b>$text</b>")
      [Windows.Forms.Clipboard]::SetDataObject($d, $true)
      return
    } catch { Start-Sleep -Milliseconds 200 }
  }
  Say 'SetClip failed'
}
function GetClip {
  for ($i = 0; $i -lt 10; $i++) {
    try {
      $d = [Windows.Forms.Clipboard]::GetDataObject()
      $text = [Windows.Forms.Clipboard]::GetText()
      $html = $null
      if ($d -and $d.GetDataPresent($HtmlFormat)) { $html = [string]$d.GetData($HtmlFormat) }
      return @{ Text = $text; Html = $html }
    } catch { Start-Sleep -Milliseconds 200 }
  }
  return @{ Text = '(clipboard unreadable)'; Html = $null }
}

# ---- environment ----
Say "user=$env:USERNAME session=$((Get-Process -Id $PID).SessionId) interactive=$([Environment]::UserInteractive) apartment=$([Threading.Thread]::CurrentThread.ApartmentState)"
Say "input desktop: $([W]::InputDesktop())"
Say "os: $((Get-CimInstance Win32_OperatingSystem).Caption) $([Environment]::OSVersion.Version)"
Say "screen: $([Windows.Forms.SystemInformation]::VirtualScreen)"
foreach ($k in 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}') {
  $v = (Get-ItemProperty -Path $k -ErrorAction SilentlyContinue).pv
  Say "webview2 $k => $v"
}
Say "visible windows at start:`r`n$([W]::ListTop(0))"
Shot '00-desktop-at-start'

# ---- mock and settings ----
$node = (Get-Command node).Source
$mockProc = Start-Process -FilePath $node -ArgumentList @((Join-Path $ProbeDir 'mock.cjs'), "`"$Out`"") -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 2
Say "mock pid=$($mockProc.Id) state=$((Rest '/_state') | ConvertTo-Json -Compress)"

$cfgDir = Join-Path $env:APPDATA 'dev.ayato.airtype'
New-Item -ItemType Directory -Force $cfgDir | Out-Null
[IO.File]::WriteAllText((Join-Path $cfgDir 'settings.json'), '{"provider":"openai","openai":{"model":"mock","baseUrl":"http://localhost:8799/v1"}}', $Utf8)

# ---- the editor ----
$Base = 'KEEP-HEAD selectme KEEP-TAIL'
$doc = Join-Path $Out 'note.txt'
[IO.File]::WriteAllText($doc, "$Base`r`n", $Utf8)
$note = Start-Process -FilePath 'notepad.exe' -ArgumentList "`"$doc`"" -PassThru
$noteTop = [IntPtr]::Zero
for ($i = 0; $i -lt 40 -and $noteTop -eq [IntPtr]::Zero; $i++) {
  Start-Sleep -Milliseconds 250
  $noteTop = [W]::FindTop(0, 'Notepad', $null, $true)
}
Say "notepad process=$($note.Id) window: $([W]::Describe($noteTop))"
Say "notepad exe: $((Get-Process notepad -ErrorAction SilentlyContinue | Select-Object -First 1).Path)"
Say "notepad children:`r`n$([W]::Children($noteTop))"
$noteEdit = [W]::FindEdit($noteTop)
Say "notepad edit: $([W]::Describe($noteEdit)) text=[$([W]::GetText($noteEdit))]"
Say "front notepad: $([W]::Front($noteTop)); foreground: $([W]::Describe([W]::GetForegroundWindow()))"
Shot '01-notepad'

# ---- the app ----
Remove-Item $AppLog -ErrorAction SilentlyContinue
$app = Start-Process -FilePath $Exe -PassThru
Say "airtype pid=$($app.Id)"
Start-Sleep -Seconds 8
Say "airtype alive=$(-not $app.HasExited) windows:`r`n$([W]::ListTop([uint32]$app.Id))"
Say "visible windows after launch:`r`n$([W]::ListTop(0))"
Shot '02-after-launch'
Say "app log: $(AppLogTail 0)"

# ---- check 5: the panel on the first try ----
$front = [W]::Front($noteTop)
Say "panel: notepad in front=$front; caret only"
Keys @('Ctrl', 'End') | Out-Null
Start-Sleep -Milliseconds 300
Shot '10-panel-0-before'
$logStart = AppLogCount
$sent = Keys @('Ctrl', 'Alt', 'Space')
$t0 = Get-Date
Say "panel: hotkey sent events=$sent"
$shots = @(0.3, 1.0, 2.0, 4.0)
$firstVisible = $null
$visibleAt = @{}
$lastVisible = $false
$panelHwnd = [IntPtr]::Zero
while (((Get-Date) - $t0).TotalSeconds -lt 6) {
  $elapsed = ((Get-Date) - $t0).TotalSeconds
  $h = [W]::FindExact([uint32]$app.Id, 'Airtype')
  $now = $h -ne [IntPtr]::Zero
  if ($now -and $null -eq $firstVisible) { $firstVisible = $elapsed; $panelHwnd = $h; Say ("panel: first visible at {0:N2}s: {1}" -f $elapsed, [W]::Describe($h)) }
  if ($lastVisible -and -not $now) { Say ("panel: went away at {0:N2}s" -f $elapsed) }
  $lastVisible = $now
  if ($shots.Count -gt 0 -and $elapsed -ge $shots[0]) {
    Shot ("11-panel-t{0}" -f $shots[0])
    Say ("panel: at {0:N2}s visible={1} foreground: {2}" -f $elapsed, $now, [W]::Describe([W]::GetForegroundWindow()))
    $shots = @($shots | Select-Object -Skip 1)
  }
  Start-Sleep -Milliseconds 60
}
if ($panelHwnd -ne [IntPtr]::Zero) { Say "panel: printwindow ok=$([W]::ShotWindow($panelHwnd, (Join-Path $Out '12-panel-window.png')))" }
$panelForeground = [W]::GetForegroundWindow()
$panelInFront = $panelHwnd -ne [IntPtr]::Zero -and $panelForeground -eq $panelHwnd
$panelLog = AppLogTail $logStart
Say "panel: app log: $panelLog"
Result ("check5 panel first_visible_s={0} still_visible_at_6s={1} in_front={2} log=[{3}]" -f $firstVisible, $lastVisible, $panelInFront, $panelLog)
$sent = Keys @('Esc')
Start-Sleep -Seconds 1
Say "panel: Esc sent=$sent visible=$([W]::FindExact([uint32]$app.Id, 'Airtype') -ne [IntPtr]::Zero) foreground: $([W]::Describe([W]::GetForegroundWindow()))"
Shot '13-panel-after-esc'
Start-Sleep -Milliseconds 800

# ---- checks 1 to 4 ----
function ResetNotepad {
  $ok = [W]::Front($noteTop)
  Keys @('Ctrl', 'A') | Out-Null
  Keys @('Del') | Out-Null
  Start-Sleep -Milliseconds 80
  [W]::TypeText($Base) | Out-Null
  Keys @('Ctrl', 'Home') | Out-Null
  for ($i = 0; $i -lt 10; $i++) { Keys @('Right') | Out-Null }
  for ($i = 0; $i -lt 8; $i++) { Keys @('Shift', 'Right') | Out-Null }
  Start-Sleep -Milliseconds 100
  return @{ Front = $ok; Field = [W]::GetText($noteEdit); Sel = 'n/a' }
}
function ReadNotepad { return [W]::GetText($noteEdit) }

function ResetPage {
  $ok = [W]::Front($script:pageTop)
  $v = [uri]::EscapeDataString($Base)
  Rest "/_set?value=$v&a=10&b=18" | Out-Null
  $st = $null
  for ($i = 0; $i -lt 30; $i++) {
    Start-Sleep -Milliseconds 100
    $st = (Rest '/_state').page
    if ($st -and $st.value -eq $Base -and $st.a -eq 10 -and $st.b -eq 18 -and $st.focus) { break }
  }
  return @{ Front = $ok; Field = $st.value; Sel = "$($st.a)-$($st.b) focus=$($st.focus)" }
}
function ReadPage { return (Rest '/_state').page.value }

function Scenario($editor, $name, $writes, $tag = '') {
  Say "===== $editor $name $tag ====="
  $prefix = "20-$editor-$name$tag"
  $sentinel = "CLIP-SENTINEL-$editor-$name$tag"
  $state = if ($editor -eq 'notepad') { ResetNotepad } else { ResetPage }
  Say "field before: [$($state.Field)] selection=$($state.Sel) front=$($state.Front) foreground: $([W]::Describe([W]::GetForegroundWindow()))"
  SetClip $sentinel
  $clipBefore = GetClip
  Say "clipboard before: text=[$($clipBefore.Text)] html=$($null -ne $clipBefore.Html)"
  $m = Rest "/_mode?m=$name"
  $expected = Norm $m.expected
  $baseline = $m.completed
  $startedBefore = $m.started
  $logStart = AppLogCount
  Shot "$prefix-0-before"
  $sent = Keys @('Ctrl', 'Alt', 'T')
  Say "hotkey sent events=$sent"
  $t0 = Get-Date
  Start-Sleep -Milliseconds 250
  Shot "$prefix-1-working"
  $hud = [W]::FindExact([uint32]$app.Id, 'Airtype HUD')
  Say "hud while working: $([W]::Describe($hud))"
  if ($hud -ne [IntPtr]::Zero) { [W]::ShotWindow($hud, (Join-Path $Out "$prefix-1-hud.png")) | Out-Null }
  $st = $null
  while (((Get-Date) - $t0).TotalSeconds -lt 12) {
    $st = Rest '/_state'
    if ($st.completed -gt $baseline) { break }
    Start-Sleep -Milliseconds 80
  }
  $served = ((Get-Date) - $t0).TotalSeconds
  Say ("mock completed={0} after {1:N2}s" -f $st.completed, $served)
  $target = "KEEP-HEAD $expected KEEP-TAIL"
  $field = $null
  if ($writes) {
    while (((Get-Date) - $t0).TotalSeconds -lt 16) {
      $field = Norm $(if ($editor -eq 'notepad') { ReadNotepad } else { ReadPage })
      if ($field -eq $target) { break }
      Start-Sleep -Milliseconds 100
    }
    Say ("field settled after {0:N2}s" -f ((Get-Date) - $t0).TotalSeconds)
  } else {
    Start-Sleep -Milliseconds 700
  }
  Shot "$prefix-2-after"
  $hud = [W]::FindExact([uint32]$app.Id, 'Airtype HUD')
  $hudUp = $hud -ne [IntPtr]::Zero
  Say "hud after answer: $([W]::Describe($hud))"
  if ($hudUp) { [W]::ShotWindow($hud, (Join-Path $Out "$prefix-2-hud.png")) | Out-Null }
  Start-Sleep -Milliseconds 1500
  $field = Norm $(if ($editor -eq 'notepad') { ReadNotepad } else { ReadPage })
  $sel = if ($editor -eq 'notepad') { 'n/a' } else { $p = (Rest '/_state').page; "$($p.a)-$($p.b)" }
  $fieldWant = if ($writes) { $target } else { $Base }
  $fieldOk = $field -ceq $fieldWant
  # The clipboard may come back in the background for up to 10 s.
  $clip = GetClip
  $clipStart = Get-Date
  while ($clip.Text -cne $sentinel -and ((Get-Date) - $clipStart).TotalSeconds -lt 11) {
    Start-Sleep -Milliseconds 250
    $clip = GetClip
  }
  $clipWait = ((Get-Date) - $clipStart).TotalSeconds
  $clipOk = $clip.Text -ceq $sentinel
  $htmlOk = ($null -ne $clip.Html) -and $clip.Html.Contains($sentinel)
  $log = AppLogTail $logStart
  $final = Rest '/_state'
  $requests = $final.started - $startedBefore
  Result ("editor={0} scenario={1}{2} field={3} clipboard={4} html={5} stream_flags={6} ({7}) selected=[{8}] requests={9} hud_after={10} selection_after={11} clip_wait_s={12:N1}" -f $editor, $name, $tag, $(if ($fieldOk) { 'PASS' } else { 'FAIL' }), $(if ($clipOk) { 'PASS' } else { 'FAIL' }), $(if ($htmlOk) { 'PASS' } else { 'FAIL' }), $(if ($final.flags -ceq $m.wantFlags) { 'PASS' } else { 'FAIL' }), $final.flags, $final.selected, $requests, $hudUp, $sel, $clipWait)
  if ($editor -ne 'notepad') { Result "   page_events=[$($final.pageEvents)]" }
  Result "   field=[$field] want=[$fieldWant]"
  Result "   clipboard=[$($clip.Text)] html=[$($clip.Html -replace '\s+', ' ')]"
  Result "   app=[$log]"
  # The slip stays for 3.8 s after an error; let the run end before the next one.
  Start-Sleep -Seconds $(if ($writes) { 1 } else { 4 })
}

$All = @(
  @('stream_line', $true), @('stream_lines', $true), @('json_line', $true), @('json_lines', $true),
  @('cut_end', $false), @('cut_reset', $false), @('refuse_stream', $true)
)
foreach ($s in $All) { Scenario 'notepad' $s[0] $s[1] }

# ---- the same in a browser text field, where the text may be read with Ctrl+C ----
$chrome = 'C:\Program Files\Google\Chrome\Application\chrome.exe'
if (Test-Path $chrome) {
  $proc = Start-Process -FilePath $chrome -ArgumentList @('--no-first-run', '--no-default-browser-check', '--disable-sync', '--user-data-dir=C:\Windows\Temp\chrome-probe', "--app=$Mock/_page") -PassThru
  $script:pageTop = [IntPtr]::Zero
  for ($i = 0; $i -lt 60 -and $script:pageTop -eq [IntPtr]::Zero; $i++) {
    Start-Sleep -Milliseconds 250
    $script:pageTop = [W]::FindTop(0, 'Chrome_WidgetWin_1', 'probe page', $true)
  }
  Say "chrome window: $([W]::Describe($script:pageTop))"
  if ($script:pageTop -ne [IntPtr]::Zero) {
    foreach ($s in $All) { Scenario 'chrome' $s[0] $s[1] }
    # Macos 26 lost one fast multi-line paste here on 7010490: how often does that happen?
    foreach ($r in 1..8) { Scenario 'chrome' 'json_lines' $true "-r$r" }
    Stop-Process -Name chrome -Force -ErrorAction SilentlyContinue
  }
} else {
  Say 'chrome not found'
}

# ---- wrap up ----
Say "airtype alive at end=$(-not $app.HasExited)"
if (Test-Path $AppLog) { Copy-Item $AppLog (Join-Path $Out 'app.log') }
Shot '99-end'
Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Id $mockProc.Id -Force -ErrorAction SilentlyContinue
Stop-Process -Name notepad -Force -ErrorAction SilentlyContinue
Say 'driver done'
exit 0
