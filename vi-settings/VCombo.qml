// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// VCombo.qml — drop-down. model: [{ value, label }]; `current` is bound to
// daemon state, `picked(value)` emitted on choice (binding rule as VSwitch).

import QtQuick
import QtQuick.Controls.Basic

ComboBox {
    id: box
    property string current: ""
    signal picked(string value)

    textRole: "label"
    valueRole: "value"
    currentIndex: {
        for (var i = 0; i < model.length; i++)
            if (model[i].value === current) return i
        return 0
    }
    onActivated: function (i) { picked(model[i].value) }

    implicitHeight: 30
    implicitWidth: 150
    font.pixelSize: Theme.fsBody
    opacity: enabled ? 1 : 0.45

    contentItem: Text {
        leftPadding: 10
        rightPadding: 26
        text: box.displayText
        color: Theme.ink
        font: box.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    indicator: Text {
        x: box.width - width - 10
        anchors.verticalCenter: parent.verticalCenter
        text: "⌄"
        color: Theme.muted
        font.pixelSize: 14
    }
    background: Rectangle {
        radius: Theme.radius
        color: box.hovered ? Theme.hover : Theme.field
        border { width: box.visualFocus ? 2 : 1
                 color: box.visualFocus ? Theme.accent : Theme.line }
    }
    delegate: ItemDelegate {
        id: item
        required property var modelData
        required property int index
        width: ListView.view.width
        height: 30
        highlighted: box.highlightedIndex === index
        contentItem: Text {
            text: item.modelData.label
            color: Theme.ink
            font.pixelSize: Theme.fsBody
            font.weight: box.currentIndex === item.index ? Font.DemiBold : Font.Normal
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: Theme.radius - 2
            color: item.highlighted ? Theme.accentSoft : "transparent"
        }
    }
    popup: Popup {
        y: box.height + 4
        width: Math.max(box.width, 160)
        padding: 4
        implicitHeight: contentItem.implicitHeight + 8
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: box.popup.visible ? box.delegateModel : null
            currentIndex: box.highlightedIndex
        }
        background: Rectangle {
            radius: Theme.radius
            color: Theme.raised
            border { width: 1; color: Theme.line }
        }
    }
}
