#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# Bộ regression FIELD-LEVEL cho vi-im (user yêu cầu 2026-07-10 sau chuỗi
# "fix lỗi này đẻ lỗi khác"). KHÔNG phải unit test: bơm phím thật ở mức
# uinput (không phân biệt được với bàn phím vật lý), đi qua daemon vi-ime
# ĐANG CHẠY, đọc kết quả từ app thật. Mọi regression 2026-07-10 (mất chữ
# 2 dấu, "quà"→"q", 'ấ' thành Enter tự gửi, Blink áp keymap trễ) đều bị
# battery này bắt được — chạy TRƯỚC KHI SHIP mọi thay đổi typing path.
#
# Yêu cầu: vi-ime đang chạy, nhóm `input`, python3-evdev, niri, zenity,
# kitty. Chạy trên session thật — KHÔNG gõ phím/chuột trong lúc chạy.
#
# Cách chạy:  bash scripts/vi-regression/run.sh
# Tuỳ chọn:   CHROME=1 bash scripts/vi-regression/run.sh   (thêm bài Chrome,
#             kết quả chụp màn hình để mắt người duyệt)
set -uo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT="${TMPDIR:-/tmp}/vi-regression-$$"
mkdir -p "$OUT"
PASS=0; FAIL=0
GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

note() { echo -e "${CYAN}── $1 ──${NC}"; }
ok()   { echo -e "  ${GREEN}PASS${NC} $1"; PASS=$((PASS+1)); }
bad()  { echo -e "  ${RED}FAIL${NC} $1"; FAIL=$((FAIL+1)); }

# paste_get: doc clipboard ra stdout (Wayland wl-paste, X11 xclip fallback)
paste_get() {
    if command -v wl-paste >/dev/null 2>&1; then
        wl-paste 2>/dev/null
    elif command -v xclip >/dev/null 2>&1; then
        xclip -selection clipboard -o 2>/dev/null
    else
        echo ""
    fi
}

# run_office: bai LibreOffice / OnlyOffice (text field nhan IME). Go KEYS
# (seq + rollover), Ctrl+A/Ctrl+C doc clipboard, diff voi WANT. Neu clipboard
# khong doc duoc -> screenshot de duyet mat (nhu Chrome).
run_office() {
    local tag="$1" sub="$2" launch="$3" mode="$4" killpat="$5"
    note "Bai $tag ($mode)"
    pkill -f "$killpat" 2>/dev/null; sleep 1
    eval "$launch" >/dev/null 2>&1 &
    sleep 7
    local wid
    wid=$(niri msg --json windows | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    t = w.get('title') or ''
    if '$sub' in t:
        print(w.get('id')); break" | head -1 | tr -d '[:space:]')
    if [ -z "$wid" ]; then
        bad "$tag: khong tim cua so (title chua '$sub')"
        return
    fi
    niri msg action focus-window --id "$wid" 2>/dev/null; sleep 1.5
    if ! focus_is "$sub"; then
        bad "$tag: khong focus duoc (wid=$wid)"
        pkill -f "$killpat" 2>/dev/null
        return
    fi
    # Xoa clipboard cu de phat hien copy that bai (tranh doc nham clipboard cu)
    if command -v wl-copy >/dev/null 2>&1; then wl-copy </dev/null 2>/dev/null; fi
    python3 "$DIR/inject.py" "$mode" "$KEYS"
    sleep 0.6
    python3 "$DIR/inject.py" shortcut 'ctrl+a'
    python3 "$DIR/inject.py" shortcut 'ctrl+c'
    sleep 0.5
    local got; got=$(paste_get | sed 's/$//' | tr -d '
')
    if [ -z "$got" ]; then
        bad "$tag/$mode: clipboard rong (focus/copy that bai - co the sai ten cua so)"
        if command -v grim >/dev/null 2>&1; then grim "$OUT/$tag-$mode.png"; echo "  -> screenshot: $OUT/$tag-$mode.png"; fi
        pkill -f "$killpat" 2>/dev/null
        return
    fi
    if [ "$got" = "$WANT" ]; then
        ok "$tag/$mode"
    else
        bad "$tag/$mode: [$got] != [$WANT]"
        if command -v grim >/dev/null 2>&1; then grim "$OUT/$tag-$mode.png"; echo "  -> screenshot: $OUT/$tag-$mode.png"; fi
    fi
    pkill -f "$killpat" 2>/dev/null
}

pgrep -x vi-ime >/dev/null || { echo "vi-ime không chạy — bật daemon trước"; exit 1; }

# Bài gõ chuẩn: quét đủ các lớp lỗi đã gặp ngoài field.
#  - từ ≥2 dấu VNI/Tự do:     ma6t1→mất  u72→ừ  d9u7o7ng2→đường  tie6m5→tiệm
#  - đổi keymap/level dồn dập: chuỗi >28 ký tự composed khác nhau
#  - hoa có dấu (level cao):   VIE6T5→VIỆT
#  - undo/sửa dấu:             qua2→quà  ke5→kẹ (từng vỡ thành "q"/"k")
KEYS="ma6t1 bo2 ca1 ge3 hi4 lu5 mo6 ne61 pa81 qua2 ri3 se4 vu7 xa6 yt1 ke5 tie6m5 u72 dau61 d9u7o7ng2 VIE6T5 "
WANT="mất bò cá gẻ hĩ lụ mô nế pắ quà rỉ sẽ vư xâ ýt kẹ tiệm ừ dấu đường VIỆT "

focus_is() {
    niri msg --json focused-window 2>/dev/null \
        | python3 -c "import json,sys; w=json.load(sys.stdin) or {}; print(w.get('title') or '')" \
        | grep -qF "$1"
}

# ── Bài 1: PREEDIT path (zenity GTK4 — commit_string chuẩn) ────────────────
run_zenity() {
    local mode="$1"
    zenity --entry --text="vi-regression" > "$OUT/zenity.txt" 2>/dev/null &
    local zpid=$!
    sleep 2
    if ! focus_is "vi-regression" && ! focus_is "Add a new entry"; then
        bad "zenity/$mode: cửa sổ không nhận focus — bỏ bài"
        kill $zpid 2>/dev/null; return
    fi
    python3 "$DIR/inject.py" "$mode" "$KEYS"
    python3 "$DIR/inject.py" seq '\n'
    wait $zpid
    local got; got=$(cat "$OUT/zenity.txt")
    if [ "$got" = "$WANT" ]; then
        ok "preedit/zenity/$mode"
    else
        bad "preedit/zenity/$mode: [$got] ≠ [$WANT]"
    fi
}

# ── Bài 2: NONPREEDIT LIVE path (kitty + cat: text thật, bắt cả tự-Enter) ──
run_kitty() {
    local mode="$1"
    rm -f "$OUT/kitty.txt"
    kitty --title vi-regression-term sh -c "cat > $OUT/kitty.txt" 2>/dev/null &
    local kpid=$!
    sleep 2.5
    local wid
    wid=$(niri msg --json windows | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if w.get('title')=='vi-regression-term': print(w.get('id'))")
    [ -n "$wid" ] && niri msg action focus-window --id "$wid"; sleep 1
    if ! focus_is "vi-regression-term"; then
        bad "kitty/$mode: cửa sổ không nhận focus — bỏ bài"
        kill $kpid 2>/dev/null; return
    fi
    python3 "$DIR/inject.py" "$mode" "$KEYS"
    sleep 0.5
    # Trước khi mình bấm Enter, file PHẢI rỗng — có byte tức là một tap bị
    # app hiểu thành Enter ('ấ' trúng keycode 28, field 2026-07-10).
    local pre; pre=$(wc -c < "$OUT/kitty.txt" 2>/dev/null || echo 0)
    python3 "$DIR/inject.py" seq '\n'
    sleep 0.5; kill $kpid 2>/dev/null; sleep 0.3
    local got; got=$(cat "$OUT/kitty.txt")
    if [ "$pre" != "0" ]; then
        bad "live/kitty/$mode: có $pre byte TRƯỚC Enter (tap bị hiểu thành Enter?)"
    elif [ "$got" = "$WANT" ]; then
        ok "live/kitty/$mode"
    else
        bad "live/kitty/$mode: [$got] ≠ [$WANT]"
    fi
}

note "Bài 1+2, gõ chậm (seq) rồi gõ nhanh kiểu người thật (rollover)"
run_zenity seq
run_zenity rollover
run_kitty seq
run_kitty rollover

# ── Bài 3 (tuỳ chọn): Chrome/Blink — kẻ khó tính nhất với keymap ───────────
if [ "${CHROME:-0}" = "1" ] && command -v grim >/dev/null; then
    note "Bài 3: Chrome textarea (kết quả = screenshot, duyệt bằng mắt)"
    CHROME_BIN=$(command -v google-chrome || command -v chromium || echo /opt/google/chrome/chrome)
    cat > "$OUT/ta.html" <<'EOF'
<title>vi-regression-chrome</title><textarea autofocus style="font-size:40px;width:95%;height:60%"></textarea>
EOF
    "$CHROME_BIN" --user-data-dir="$OUT/chrome-profile" --no-first-run \
        --ozone-platform=wayland --enable-wayland-ime --wayland-text-input-version=3 \
        "file://$OUT/ta.html" >/dev/null 2>&1 &
    sleep 5
    wid=$(niri msg --json windows | python3 -c "
import json,sys
for w in json.load(sys.stdin):
    if 'vi-regression-chrome' in (w.get('title') or ''): print(w.get('id'))")
    if [ -n "$wid" ]; then
        niri msg action focus-window --id "$wid"; sleep 1
        python3 "$DIR/inject.py" rollover "$KEYS"
        sleep 1
        grim "$OUT/chrome.png"
        echo "  → duyệt bằng mắt: $OUT/chrome.png (kỳ vọng: \"$WANT\", không gạch chân nếu nonpreedit)"
        niri msg action close-window --id "$wid"
    else
        bad "chrome: không mở được cửa sổ test"
    fi
    pkill -f "$OUT/chrome-profile" 2>/dev/null
fi

# Bai 4+5 (tuy chon): LibreOffice / OnlyOffice - app kho tinh nhat voi
# keymap/evdev. Bat bang LO=1 / OO=1 (can app cai san + niri + wl-paste/xclip).
if [ "${LO:-0}" = "1" ]; then
    run_office "libreoffice" "LibreOffice Writer" "libreoffice --writer --nologo" seq "libreoffice"
    run_office "libreoffice" "LibreOffice Writer" "libreoffice --writer --nologo" rollover "libreoffice"
fi
if [ "${OO:-0}" = "1" ]; then
    run_office "onlyoffice" "ONLYOFFICE" "onlyoffice-desktopeditors" seq "onlyoffice-desktopeditors"
    run_office "onlyoffice" "ONLYOFFICE" "onlyoffice-desktopeditors" rollover "onlyoffice-desktopeditors"
fi

echo
note "KẾT QUẢ: $PASS pass, $FAIL fail (log: $OUT)"
[ "$FAIL" = "0" ]
