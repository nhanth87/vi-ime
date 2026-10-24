// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// Segmented.qml — one-of-N picker for 2-3 short options.
// model: [{ value, label }]; `current` is bound to daemon state and only
// `picked(value)` is emitted on click (same binding rule as VSwitch).

import QtQuick

Rectangle {
    id: seg
    property var model: []
    property string current: ""
    property int segmentWidth: 0     // 0 = size to label
    signal picked(string value)

    implicitWidth: row.implicitWidth + 4
    implicitHeight: 30
    radius: Theme.radius
    color: Theme.field
    border { width: 1; color: Theme.line }
    opacity: enabled ? 1 : 0.45

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 2
        Repeater {
            model: seg.model
            delegate: Rectangle {
                id: cell
                required property var modelData
                readonly property bool active: seg.current === modelData.value
                width: seg.segmentWidth > 0 ? seg.segmentWidth : t.implicitWidth + 22
                height: seg.height - 4
                radius: Theme.radius - 2
                color: active ? Theme.raised : (ma.containsMouse ? Theme.hover : "transparent")
                border { width: active ? 1 : 0; color: Theme.line }
                activeFocusOnTab: true
                Accessible.role: Accessible.RadioButton
                Accessible.name: modelData.label
                Accessible.checked: active
                Keys.onSpacePressed: seg.picked(modelData.value)
                Keys.onReturnPressed: seg.picked(modelData.value)

                Text {
                    id: t
                    anchors.centerIn: parent
                    text: cell.modelData.label
                    color: cell.active ? Theme.ink : Theme.muted
                    font { pixelSize: Theme.fsBody; weight: cell.active ? Font.DemiBold : Font.Normal }
                }
                Rectangle {
                    anchors { fill: parent; margins: -2 }
                    radius: Theme.radius
                    color: "transparent"
                    border { width: 2; color: Theme.accent }
                    visible: cell.activeFocus
                }
                MouseArea {
                    id: ma
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: seg.picked(cell.modelData.value)
                }
            }
        }
    }
}
