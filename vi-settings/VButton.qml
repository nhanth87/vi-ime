// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// VButton.qml — push button. kind: "normal" | "primary" | "danger" | "ghost".

import QtQuick

Rectangle {
    id: btn
    property string text: ""
    property string kind: "normal"
    signal clicked

    implicitWidth: label.implicitWidth + 28
    implicitHeight: 32
    radius: Theme.radius
    activeFocusOnTab: true
    opacity: enabled ? 1 : 0.45

    readonly property bool primary: kind === "primary"
    color: primary ? (ma.pressed ? Qt.darker(Theme.accent, 1.15)
                                 : ma.containsMouse ? Qt.lighter(Theme.accent, 1.08) : Theme.accent)
         : kind === "ghost" ? (ma.containsMouse ? Theme.hover : "transparent")
         : kind === "danger" ? (ma.containsMouse ? Theme.offSoft : "transparent")
         : (ma.containsMouse ? Theme.hover : Theme.raised)
    border.width: (kind === "normal" || kind === "danger") ? 1 : 0
    border.color: kind === "danger" ? Qt.rgba(0.878, 0.29, 0.29, 0.5) : Theme.line

    Accessible.role: Accessible.Button
    Accessible.name: text
    Keys.onSpacePressed: if (enabled) clicked()
    Keys.onReturnPressed: if (enabled) clicked()

    Text {
        id: label
        anchors.centerIn: parent
        text: btn.text
        color: btn.primary ? Theme.accentInk : btn.kind === "danger" ? Theme.off : Theme.ink
        font { pixelSize: Theme.fsBody; weight: btn.primary ? Font.DemiBold : Font.Normal }
    }
    Rectangle {
        anchors { fill: parent; margins: -3 }
        radius: Theme.radius + 2
        color: "transparent"
        border { width: 2; color: Theme.accent }
        visible: btn.activeFocus
    }
    MouseArea {
        id: ma
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: btn.clicked()
    }
}
