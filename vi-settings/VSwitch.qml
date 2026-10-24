// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// VSwitch.qml — on/off switch. `checked` is meant to be BOUND to daemon state;
// clicking only emits `toggled(next)` and never assigns `checked` itself —
// assigning would break the binding and the switch would stop following
// changes made from the tray or CLI.

import QtQuick

Item {
    id: sw
    property bool checked: false
    property string accessibleName: ""
    signal toggled(bool next)

    implicitWidth: 38; implicitHeight: 22
    activeFocusOnTab: true
    opacity: enabled ? 1 : 0.45

    Accessible.role: Accessible.CheckBox
    Accessible.name: accessibleName
    Accessible.checked: checked
    Keys.onSpacePressed: if (enabled) toggled(!checked)
    Keys.onReturnPressed: if (enabled) toggled(!checked)

    Rectangle {
        id: track
        anchors.fill: parent
        radius: height / 2
        color: sw.checked ? Theme.accent : (Theme.dark ? "#424A55" : "#C3CAD4")
        Behavior on color { ColorAnimation { duration: 120 } }

        Rectangle {
            width: parent.height - 6; height: width; radius: width / 2
            y: 3
            x: sw.checked ? parent.width - width - 3 : 3
            color: "#FFFFFF"
            Behavior on x { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
    }
    Rectangle {
        anchors { fill: parent; margins: -3 }
        radius: height / 2
        color: "transparent"
        border { width: 2; color: Theme.accent }
        visible: sw.activeFocus
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: { sw.forceActiveFocus(); sw.toggled(!sw.checked) }
    }
}
