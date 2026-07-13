#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Bơm phím mức uinput (như bàn phím vật lý) cho bộ regression vi-im.
# Modes:
#   seq      — tuần tự, release trước press kế (gõ chậm 80ms/phím)
#   rollover — gõ nhanh kiểu người thật: press phím kế TRƯỚC khi release
#              phím trước (20ms/phím) — đây là mode đã bắt được mọi
#              regression field 2026-07-10
# Chữ HOA trong chuỗi → tự kèm Shift. `\n` = Enter, `\b` = BackSpace.
import sys
import time

import evdev
from evdev import ecodes as e

KC = {
    'a': 30, 'b': 48, 'c': 46, 'd': 32, 'e': 18, 'f': 33, 'g': 34, 'h': 35,
    'i': 23, 'j': 36, 'k': 37, 'l': 38, 'm': 50, 'n': 49, 'o': 24, 'p': 25,
    'q': 16, 'r': 19, 's': 31, 't': 20, 'u': 22, 'v': 47, 'w': 17, 'x': 45,
    'y': 21, 'z': 44,
    '1': 2, '2': 3, '3': 4, '4': 5, '5': 6, '6': 7, '7': 8, '8': 9,
    '9': 10, '0': 11, ' ': 57, '\n': 28, '\b': 14,
}
SHIFT = 42


def main() -> int:
    mode = sys.argv[1]
    text = sys.argv[2].replace('\\n', '\n').replace('\\b', '\b')
    caps = {e.EV_KEY: sorted(set(KC.values()) | {SHIFT})}
    ui = evdev.UInput(caps, name='vi-regression-kbd')
    # Chờ compositor/IME nhận thiết bị mới trước khi gõ.
    time.sleep(1.5)
    try:
        if mode == 'seq':
            for c in text:
                kc = KC[c.lower()] if c.isalpha() else KC[c]
                sh = c.isupper()
                if sh:
                    ui.write(e.EV_KEY, SHIFT, 1); ui.syn(); time.sleep(0.01)
                ui.write(e.EV_KEY, kc, 1); ui.syn(); time.sleep(0.02)
                ui.write(e.EV_KEY, kc, 0); ui.syn()
                if sh:
                    ui.write(e.EV_KEY, SHIFT, 0); ui.syn()
                time.sleep(0.08)
        elif mode == 'rollover':
            prev = None
            for c in text:
                kc = KC[c.lower()] if c.isalpha() else KC[c]
                sh = c.isupper()
                if sh:
                    ui.write(e.EV_KEY, SHIFT, 1); ui.syn(); time.sleep(0.005)
                ui.write(e.EV_KEY, kc, 1); ui.syn(); time.sleep(0.008)
                if prev is not None:
                    ui.write(e.EV_KEY, prev, 0); ui.syn()
                if sh:
                    ui.write(e.EV_KEY, SHIFT, 0); ui.syn()
                prev = kc
                time.sleep(0.012)
            if prev is not None:
                ui.write(e.EV_KEY, prev, 0); ui.syn()
        elif mode == 'shortcut':
            # tokens: "ctrl+a" "ctrl+c" "shift+Home" ... gửi modifier+key
            MODS = {'ctrl': 29, 'shift': 42, 'alt': 56}
            for tok in text.split():
                if '+' not in tok:
                    continue
                mod, key = tok.split('+', 1)
                mcode = MODS.get(mod.lower())
                kc = KC.get(key.lower())
                if kc is None:
                    continue
                if mcode is not None:
                    ui.write(e.EV_KEY, mcode, 1); ui.syn(); time.sleep(0.01)
                ui.write(e.EV_KEY, kc, 1); ui.syn(); time.sleep(0.02)
                ui.write(e.EV_KEY, kc, 0); ui.syn()
                if mcode is not None:
                    ui.write(e.EV_KEY, mcode, 0); ui.syn()
                time.sleep(0.05)
        else:
            print(f'unknown mode {mode}', file=sys.stderr)
            return 2
        time.sleep(0.5)
        return 0
    finally:
        ui.close()


if __name__ == '__main__':
    sys.exit(main())
