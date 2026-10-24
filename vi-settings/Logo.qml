// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// Logo.qml — the round tray glyph (blue V = on, red E = off), drawn in QML so
// it stays crisp at any size. Mirrors assets/icons/vi-im{,-off}.svg.

import QtQuick

Rectangle {
    property bool on: true
    property int size: 32
    width: size; height: size; radius: size / 2
    color: on ? Theme.accent : Theme.off
    Behavior on color { ColorAnimation { duration: 140 } }

    Text {
        anchors.centerIn: parent
        anchors.verticalCenterOffset: size * 0.02
        text: parent.on ? "V" : "E"
        color: "#FFFFFF"
        font { pixelSize: Math.round(parent.size * 0.54); bold: true }
    }
}
