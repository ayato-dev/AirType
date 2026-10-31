#!/bin/bash
# Drives the real Airtype on a GitHub-hosted macOS runner. Args: app bundle, output folder, probe folder.
APP="$1"; OUT="$2"; PROBE="$3"
mkdir -p "$OUT"
export LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8
MOCK=http://localhost:8799
APPLOG=/tmp/airtype-probe.log
BASE='KEEP-HEAD selectme KEEP-TAIL'

say() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$OUT/driver.log"; }
result() { echo "$*" >> "$OUT/results.txt"; say "RESULT $*"; }
lim() { perl -e 'alarm shift; exec @ARGV' "$@"; }
shot() { lim 15 screencapture -x "$OUT/$1.png" || say "shot $1 failed"; }
se() { lim 20 osascript -e "tell application \"System Events\" to $1"; }
rest() { curl -s --max-time 5 "$MOCK$1"; }
field_of() { node -e 'const j=JSON.parse(require("fs").readFileSync(0,"utf8"));const v=process.argv[1].split(".").reduce((o,k)=>o==null?o:o[k],j);process.stdout.write(v==null?"":String(v))' "$1"; }
norm() { tr '\r' '\n' | sed -e 's/[[:space:]]*$//' ; }
applog_count() { [ -f "$APPLOG" ] && wc -l < "$APPLOG" | tr -d ' ' || echo 0; }
applog_since() { [ -f "$APPLOG" ] && tail -n +"$(( $1 + 1 ))" "$APPLOG" | tr '\n' '|' ; }

# ---- permissions: see the macOS probe recipe ----
NOW=$(date +%s)
DB="/Library/Application Support/com.apple.TCC/TCC.db"
grant() { sudo sqlite3 "$DB" "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version, flags, last_modified) VALUES ('$1','$2',$3,2,4,1,0,$NOW);" || say "grant failed: $1 $2"; }
grant kTCCServiceAccessibility dev.ayato.airtype 0
grant kTCCServiceAccessibility /usr/bin/osascript 1
grant kTCCServiceScreenCapture /usr/sbin/screencapture 1
grant kTCCServiceScreenCapture /bin/bash 1
sudo pkill -9 tccd
sleep 2

say "os: $(sw_vers -productVersion) screen: $(system_profiler SPDisplaysDataType 2>/dev/null | grep -i resolution | head -1)"
shot 00-desktop-at-start

# ---- mock and settings ----
node "$PROBE/mock.cjs" "$OUT" > "$OUT/mock.out" 2>&1 &
MOCK_PID=$!
sleep 2
say "mock state: $(rest /_state)"
CFG="$HOME/Library/Application Support/dev.ayato.airtype"
mkdir -p "$CFG"
printf '%s' '{"provider":"openai","openai":{"model":"mock","baseUrl":"http://localhost:8799/v1"}}' > "$CFG/settings.json"

# ---- the editor ----
DOC="$OUT/note.txt"
printf '%s\n' "$BASE" > "$DOC"
lim 20 open -a TextEdit "$DOC"
sleep 3
TA='text area 1 of scroll area 1 of window 1'
say "textedit windows: $(se 'tell process "TextEdit" to get name of every window')"
se 'set frontmost of process "TextEdit" to true'
sleep 1
say "textedit text: [$(se "tell process \"TextEdit\" to get value of $TA")]"
shot 01-textedit

# ---- the app ----
rm -f "$APPLOG"
lim 20 open -n "$APP" --stdout "$OUT/app.stdout" --stderr "$OUT/app.stderr"
sleep 8
say "airtype: $(pgrep -lf 'Airtype.app/Contents/MacOS' | head -3)"
say "airtype windows: $(se 'tell process "airtype" to get name of every window' 2>&1)"
shot 02-after-launch
say "app log: $(applog_since 0)"
say "front process: $(se 'get name of first process whose frontmost is true')"

# The process name in System Events follows the bundle.
PROC=$(se 'get name of every process whose unix id is '"$(pgrep -f 'Airtype.app/Contents/MacOS' | head -1)")
say "airtype process name: [$PROC]"

# ---- check 5: the panel on the first try ----
se 'set frontmost of process "TextEdit" to true'
sleep 0.5
lim 20 osascript -e 'tell application "System Events" to key code 125 using command down'
sleep 0.4
shot 10-panel-0-before
START=$(applog_count)
se 'keystroke "a" using {command down, option down}'
say 'panel: hotkey sent'
for at in 0.3 1.0 2.0 4.0; do
  sleep "$at"
  shot "11-panel-t$at"
  say "panel: +$at windows: $(se "tell process \"$PROC\" to get {name, position, size} of every window" 2>&1) front: $(se 'get name of first process whose frontmost is true')"
done
sleep 1
PANEL_WINDOWS=$(se "tell process \"$PROC\" to get name of every window" 2>&1)
PANEL_FRONT=$(se 'get name of first process whose frontmost is true')
result "check5 panel windows=[$PANEL_WINDOWS] front=[$PANEL_FRONT] log=[$(applog_since "$START")]"
lim 20 osascript -e 'tell application "System Events" to key code 53'
sleep 1
say "panel: after Esc windows: $(se "tell process \"$PROC\" to get name of every window" 2>&1) front: $(se 'get name of first process whose frontmost is true')"
shot 13-panel-after-esc
sleep 1

# ---- checks 1 to 4 ----
reset_textedit() {
  se 'set frontmost of process "TextEdit" to true'
  sleep 0.3
  se "tell process \"TextEdit\" to set value of $TA to \"$BASE\""
  sleep 0.2
  lim 25 osascript <<'EOF'
tell application "System Events"
  key code 126 using command down
  repeat 10 times
    key code 124
  end repeat
  repeat 8 times
    key code 124 using shift down
  end repeat
end tell
EOF
  sleep 0.2
  say "field before: [$(se "tell process \"TextEdit\" to get value of $TA")] selected=[$(se "tell process \"TextEdit\" to get value of attribute \"AXSelectedText\" of $TA")] front=[$(se 'get name of first process whose frontmost is true')]"
}
read_textedit() { se "tell process \"TextEdit\" to get value of $TA" | norm; }

reset_page() {
  se 'set frontmost of process "Google Chrome" to true'
  sleep 0.3
  rest "/_set?value=KEEP-HEAD%20selectme%20KEEP-TAIL&a=10&b=18" > /dev/null
  sleep 0.5
  say "field before: $(rest /_state) front=[$(se 'get name of first process whose frontmost is true')]"
}
read_page() { rest /_state | field_of page.value | norm; }

scenario() {
  local editor="$1" name="$2" writes="$3" tag="${4:-}"
  say "===== $editor $name $tag ====="
  local prefix="20-$editor-$name$tag" sentinel="CLIP-SENTINEL-$editor-$name$tag"
  "reset_$editor"
  printf '%s' "$sentinel" | pbcopy
  say "clipboard before: [$(pbpaste)]"
  local m expected want_flags baseline started log_start
  m=$(rest "/_mode?m=$name&delay=${DELAY:-300}")
  expected=$(printf '%s' "$m" | field_of expected)
  want_flags=$(printf '%s' "$m" | field_of wantFlags)
  baseline=$(printf '%s' "$m" | field_of completed)
  started=$(printf '%s' "$m" | field_of started)
  log_start=$(applog_count)
  [ -n "${STRESS:-}" ] || shot "$prefix-0-before"
  se 'keystroke "t" using {command down, option down}'
  say 'hotkey sent'
  sleep 0.25
  [ -n "${STRESS:-}" ] || shot "$prefix-1-working"
  local waited=0
  while [ "$waited" -lt 120 ]; do
    [ "$(rest /_state | field_of completed)" -gt "$baseline" ] && break
    sleep 0.1
    waited=$((waited + 1))
  done
  say "mock completed after ~$waited ticks"
  local want field
  if [ "$writes" = yes ]; then
    want="KEEP-HEAD $expected KEEP-TAIL"
    local tries=0
    while [ "$tries" -lt 25 ]; do
      field="$("read_$editor")"
      [ "$field" = "$want" ] && break
      sleep 0.2
      tries=$((tries + 1))
    done
  else
    want="$BASE"
    sleep 0.7
  fi
  shot "$prefix-2-after"
  say "hud/windows after answer: $(se "tell process \"$PROC\" to get name of every window" 2>&1)"
  sleep 1.5
  field="$("read_$editor")"
  local clip fok=FAIL cok=FAIL
  clip="$(pbpaste)"
  local tries=0
  while [ "$clip" != "$sentinel" ] && [ "$tries" -lt 20 ]; do
    sleep 0.5
    clip="$(pbpaste)"
    tries=$((tries + 1))
  done
  [ "$field" = "$want" ] && fok=PASS
  [ "$clip" = "$sentinel" ] && cok=PASS
  local final flags fl_ok=FAIL
  final=$(rest /_state)
  flags=$(printf '%s' "$final" | field_of flags)
  [ "$flags" = "$want_flags" ] && fl_ok=PASS
  local requests=$(( $(printf '%s' "$final" | field_of started) - started ))
  result "editor=$editor scenario=$name$tag field=$fok clipboard=$cok stream_flags=$fl_ok ($flags) selected=[$(printf '%s' "$final" | field_of selected)] requests=$requests clip_extra_wait_halfsec=$tries"
  [ "$editor" = page ] && result "   page_events=[$(printf '%s' "$final" | field_of pageEvents)]"
  result "   field=[$field] want=[$want]"
  result "   clipboard=[$clip]"
  result "   app=[$(applog_since "$log_start")]"
  if [ "$writes" = yes ]; then sleep 1; else sleep 4; fi
}

ALL="stream_line:yes stream_lines:yes json_line:yes json_lines:yes cut_end:no cut_reset:no refuse_stream:yes"
[ -n "${STRESS:-}" ] || for s in $ALL; do scenario textedit "${s%%:*}" "${s##*:}"; done

# ---- the same in a browser text field: there the answer goes in with keys, not through AX ----
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
if [ -x "$CHROME" ]; then
  "$CHROME" --no-first-run --no-default-browser-check --disable-sync --user-data-dir=/tmp/chrome-probe "--app=$MOCK/_page" > "$OUT/chrome.log" 2>&1 &
  CHROME_PID=$!
  sleep 6
  se 'set frontmost of process "Google Chrome" to true'
  sleep 1
  say "chrome: page state $(rest /_state)"
  shot 19-chrome
  [ -n "${STRESS:-}" ] || for s in $ALL; do scenario page "${s%%:*}" "${s##*:}"; done
  # macOS 26 lost one fast multi-line paste here on 7010490: how often does that happen?
  if [ -n "${STRESS:-}" ]; then
    # Cmd+V with Airtype idle: does this page lose pastes by itself?
    CONTROL_TEXT='CONTROL строка 1
CONTROL строка 2'
    control() {
      reset_page > /dev/null
      printf '%s' "$CONTROL_TEXT" | pbcopy
      rest "/_mode?m=json_lines" > /dev/null
      se 'keystroke "v" using {command down}'
      sleep 0.8
      local field fok=FAIL final
      field="$(read_page)"
      [ "$field" = "KEEP-HEAD $CONTROL_TEXT KEEP-TAIL" ] && fok=PASS
      final=$(rest /_state)
      result "editor=page scenario=control-$1 field=$fok page_events=[$(printf '%s' "$final" | field_of pageEvents)]"
      [ "$fok" = PASS ] || { result "   field=[$field]"; shot "30-control-$1-fail"; }
    }
    [ "$STRESS" = main ] || for r in $(seq 1 60); do control "$r"; done
    DELAY=2000
    for r in $(seq 1 40); do scenario page json_lines yes "-slow$r"; done
    DELAY=300
    for r in $(seq 1 40); do scenario page json_lines yes "-fast$r"; done
    # The first paste after Chrome starts failed on both Macs: again, with and without Airtype.
    [ "$STRESS" = main ] || for r in 1 2 3 4 5 6; do
      kill "$CHROME_PID" 2>/dev/null; sleep 2
      "$CHROME" --no-first-run --no-default-browser-check --disable-sync --user-data-dir=/tmp/chrome-probe "--app=$MOCK/_page" > "$OUT/chrome.log" 2>&1 &
      CHROME_PID=$!
      sleep 6
      if [ $((r % 2)) = 1 ]; then scenario page json_lines yes "-first$r"; else control "first$r"; fi
    done
  else
    for r in 1 2 3 4 5 6 7 8; do scenario page json_lines yes "-r$r"; done
  fi
  kill "$CHROME_PID" 2>/dev/null
else
  say 'chrome not found'
fi

# ---- wrap up ----
say "airtype alive at end: $(pgrep -lf 'Airtype.app/Contents/MacOS' | head -1)"
[ -f "$APPLOG" ] && cp "$APPLOG" "$OUT/app.log"
shot 99-end
pkill -f 'Airtype.app/Contents/MacOS'
kill "$MOCK_PID" 2>/dev/null
say 'driver done'
exit 0
