// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// Theme.qml — palette + type scale shared by main.qml (config window) and
// menu.qml (tray quick menu). Follows the desktop light/dark preference the
// way fcitx5's config tool does; the accent is the tray icon's blue so the
// window, the popup and the panel glyph read as one product.

pragma Singleton
import QtQuick

QtObject {
    readonly property bool dark: Qt.styleHints.colorScheme === Qt.ColorScheme.Dark

    // Brand: same values as scripts/gen-icons.py (BG_ON / BG_OFF).
    readonly property color accent:     "#2E86DE"
    readonly property color accentInk:  "#FFFFFF"
    readonly property color accentSoft: dark ? Qt.rgba(0.18, 0.525, 0.871, 0.24)
                                             : Qt.rgba(0.18, 0.525, 0.871, 0.12)
    readonly property color off:        "#E04A4A"
    readonly property color offSoft:    Qt.rgba(0.878, 0.29, 0.29, dark ? 0.22 : 0.10)
    readonly property color ok:         dark ? "#5CC48A" : "#23864F"

    // Surfaces: sidebar is the darker plane, content the lighter one.
    readonly property color window:  dark ? "#1C2026" : "#F2F4F7"
    readonly property color sidebar: dark ? "#16191E" : "#E7EBF0"
    readonly property color surface: dark ? "#22272E" : "#FFFFFF"
    readonly property color raised:  dark ? "#2A3038" : "#FFFFFF"
    readonly property color hover:   dark ? "#2C323B" : "#EEF2F6"
    readonly property color line:    dark ? "#343B45" : "#DCE1E8"
    readonly property color field:   dark ? "#1A1E24" : "#F7F8FA"

    // Ink.
    readonly property color ink:   dark ? "#E8EBEF" : "#18212C"
    readonly property color muted: dark ? "#9AA4B1" : "#5A6472"
    readonly property color faint: dark ? "#66707C" : "#98A1AD"

    // Type scale (px). System UI font on purpose: a config tool should look
    // like the rest of the desktop, as fcitx5-configtool does.
    readonly property int fsSmall:   12
    readonly property int fsBody:    13
    readonly property int fsLabel:   14
    readonly property int fsTitle:   20

    readonly property int radius: 6
    readonly property int rowHeight: 56
}
