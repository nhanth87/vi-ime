#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# One-shot verify cho các fix 2026-07-12 (meèo/me2o + tắt-bộ-gõ + R18).
# Chạy MỘT lệnh, làm theo lời nhắc, rồi dán TOÀN BỘ output lại cho agent.
#
#   bash scripts/verify-fixes.sh
#
# Script tự: kill daemon cũ → chạy binary MỚI (target/release) với log →
# hướng dẫn bạn 3 phép thử tay → gom log liên quan ra cuối cho dễ dán.
# KHÔNG bơm phím tự động (bug ở address bar cần mắt người duyệt).
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/vi-ime"
LOG="${TMPDIR:-/tmp}/vi-verify.log"

[ -x "$BIN" ] || { echo "❌ chưa build: chạy 'cargo build -p vi-daemon --release' trước"; exit 1; }

echo "── 1. Dừng daemon cũ ──"
pkill -x vi-ime 2>/dev/null && sleep 1 || echo "  (không có daemon nào đang chạy)"

echo "── 2. Chạy binary MỚI (log → $LOG) ──"
setsid "$BIN" >"$LOG" 2>&1 </dev/null &
disown
sleep 2
pgrep -x vi-ime >/dev/null && echo "  ✅ daemon mới đang chạy" || { echo "  ❌ daemon không lên — xem $LOG"; tail -20 "$LOG"; exit 1; }

cat <<'EOF'

── 3. TỰ THỬ TAY (theo đúng thứ tự) ───────────────────────────────
  A. BẬT bộ gõ. Vào address bar CHROME, gõ VNI: m e 2 o
     → mong đợi:  mèo     (KHÔNG phải meèo)
  B. Address bar FIREFOX, gõ: m e 2 o
     → mong đợi:  mèo     (KHÔNG phải me2o)
  C. Ô text thường (vd ô search giữa trang), gõ: m e 2 o
     → mong đợi:  mèo
  D. TẮT bộ gõ (tray hoặc: vi-ime --toggle). Vào Chrome gõ: win
     → mong đợi:  win     (tiếng Anh thô, KHÔNG ra tiếng Việt)
  E. Đang gõ dở "me" thì TẮT giữa chừng
     → mong đợi:  không có tiếng Việt nào bị phun ra

Gõ xong hết, nhấn ENTER ở đây để gom log...
EOF
read -r _

echo "── 4. LOG liên quan (copy toàn bộ khối dưới đây gửi agent) ──"
echo "=================== BEGIN LOG ==================="
grep -aE "KEY-IN|COMMIT|CONTENT-TYPE|SCENARIO|LEGACY-GRAB|XWAYLAND|EVDEV|RECONFIG|disabled|enabled" "$LOG" | tail -80
echo "==================== END LOG ===================="
echo
echo "Ghi kèm kết quả A–E (ra chữ gì thực tế) rồi dán tất cả cho agent."
