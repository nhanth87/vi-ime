// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// VField.qml — single-line text input, styled to match VButton's height.

import QtQuick
import QtQuick.Controls.Basic

TextField {
    implicitHeight: 32
    leftPadding: 10; rightPadding: 10
    color: Theme.ink
    placeholderTextColor: Theme.faint
    selectionColor: Theme.accentSoft
    selectedTextColor: Theme.ink
    font.pixelSize: Theme.fsBody
    verticalAlignment: TextInput.AlignVCenter
    background: Rectangle {
        radius: Theme.radius
        color: Theme.field
        border { width: parent.activeFocus ? 2 : 1
                 color: parent.activeFocus ? Theme.accent : Theme.line }
    }
}
