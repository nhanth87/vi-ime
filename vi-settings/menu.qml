// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// menu.qml — vi-im quick menu, opened by middle-clicking the tray icon
// (`vi-settings --menu`).
//
// Why a full-screen layer surface: on Wayland a client cannot ask where the
// pointer is, and the AppIndicator "secondary activate" carries no position.
// A transparent overlay on every screen receives wl_pointer.enter with the
// cursor's surface coordinates as soon as it maps, so the card can open right
// beside the tray icon that was just clicked. If no pointer position arrives
// (some compositors only send enter on the next motion) the card falls back
// to the top-right corner, where most bars keep their tray.
//
// Clicking outside the card, Escape, or picking an action closes the menu
// (the process exits — vi-settings relaunches it on the next middle-click).

import Quickshell
import Quickshell.Wayland
import QtQuick
import QtQuick.Layouts

ShellRoot {
    id: root

    Daemon { id: daemon }

    // Which screen claimed the pointer, and where (screen-local coordinates).
    property var claimedBy: null
    property point anchorPos: Qt.point(-1, -1)

    function close() { Qt.quit() }

    Timer {   // fallback when no screen reports the pointer
        interval: 160; running: true
        onTriggered: if (root.claimedBy === null) root.claimedBy = "fallback"
    }

    Variants {
        model: Quickshell.screens

        PanelWindow {
            id: overlay
            required property var modelData
            screen: modelData
            anchors { top: true; bottom: true; left: true; right: true }
            exclusionMode: ExclusionMode.Ignore
            color: "transparent"
            WlrLayershell.layer: WlrLayer.Overlay
            WlrLayershell.namespace: "vi-im-menu"
            WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive

            readonly property bool owner: root.claimedBy === overlay
                || (root.claimedBy === "fallback" && modelData === Quickshell.screens[0])

            MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.AllButtons
                onPositionChanged: function (m) {
                    if (root.claimedBy === null) {
                        root.anchorPos = Qt.point(m.x, m.y)
                        root.claimedBy = overlay
                    }
                }
                onPressed: root.close()
            }

            Item {
                anchors.fill: parent
                focus: true
                Keys.onEscapePressed: root.close()
            }

            MenuCard {
                id: card
                visible: overlay.owner
                // Beside the pointer, flipped above it when near the bottom,
                // clamped inside the screen with an 8px margin.
                readonly property bool hasPos: root.claimedBy !== "fallback" && root.anchorPos.x >= 0
                x: hasPos ? Math.max(8, Math.min(root.anchorPos.x - width / 2, overlay.width - width - 8))
                          : overlay.width - width - 12
                y: !hasPos ? 12
                   : root.anchorPos.y > overlay.height / 2
                     ? Math.max(8, root.anchorPos.y - height - 14)
                     : Math.min(root.anchorPos.y + 14, overlay.height - height - 8)
                onVisibleChanged: if (visible) show()
            }
        }
    }

    // ── Card ──────────────────────────────────────────────────────
    component MenuCard: Rectangle {
        id: mc
        property Item firstRow: null
        function show() {
            open.restart()
            if (firstRow) firstRow.forceActiveFocus()
        }
        width: 296
        height: col.implicitHeight + 12
        radius: 10
        color: Theme.raised
        border { width: 1; color: Theme.line }
        opacity: 0
        scale: 0.97
        transformOrigin: Item.Top

        ParallelAnimation {
            id: open
            NumberAnimation { target: mc; property: "opacity"; to: 1; duration: 110 }
            NumberAnimation { target: mc; property: "scale"; to: 1; duration: 140; easing.type: Easing.OutCubic }
        }
        MouseArea { anchors.fill: parent }   // clicks on the card never close it

        ColumnLayout {
            id: col
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 6 }
            spacing: 0

            // Header: state + master switch.
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 10
                spacing: 12
                Logo { size: 34; on: daemon.imeEnabled || !daemon.connected }
                ColumnLayout {
                    spacing: 1
                    Layout.fillWidth: true
                    Text {
                        text: !daemon.connected ? "vi-im chưa chạy"
                              : daemon.imeEnabled ? "Tiếng Việt" : "Tiếng Anh"
                        color: Theme.ink
                        font { pixelSize: Theme.fsLabel; weight: Font.DemiBold }
                    }
                    Text {
                        text: !daemon.connected ? "Không kết nối được bộ gõ"
                              : !daemon.loaded ? "Đang đọc trạng thái…"
                              : daemon.inputMethod + ", "
                                + (daemon.imeMode === "Preedit" ? "gạch chân khi gõ" : "gõ thẳng")
                        color: Theme.muted
                        font.pixelSize: Theme.fsSmall
                    }
                }
                VSwitch {
                    id: masterSwitch
                    checked: daemon.imeEnabled
                    enabled: daemon.loaded
                    accessibleName: "Gõ tiếng Việt"
                    onToggled: function (next) { daemon.setConfig({ enabled: next }) }
                    KeyNavigation.down: mc.firstRow
                }
            }

            Divider {}
            Caption { text: "Kiểu gõ" }
            Repeater {
                model: [
                    { value: "Telex", hint: "aa → â" },
                    { value: "VNI",   hint: "a6 → â" },
                    { value: "Tự do", hint: "Telex + VNI" }
                ]
                delegate: MenuRow {
                    required property var modelData
                    required property int index
                    text: modelData.value
                    hint: modelData.hint
                    checkable: true
                    checked: daemon.loaded && daemon.inputMethod === modelData.value
                    enabled: daemon.loaded
                    onActivated: daemon.setConfig({ input_method: modelData.value })
                    Component.onCompleted: if (index === 0) mc.firstRow = this
                }
            }

            Divider {}
            OptionRow {
                text: "Hiển thị"
                Segmented {
                    model: [{ value: "Preedit", label: "Gạch chân" },
                            { value: "NonPreedit", label: "Gõ thẳng" }]
                    current: daemon.loaded ? daemon.imeMode : ""
                    enabled: daemon.loaded
                    onPicked: function (v) { daemon.setConfig({ ime_mode: v }) }
                }
            }
            OptionRow {
                text: "Vị trí dấu"
                Segmented {
                    model: [{ value: "classic", label: "hòa" }, { value: "modern", label: "hoà" }]
                    current: daemon.loaded ? daemon.toneStyle : ""
                    segmentWidth: 54
                    enabled: daemon.loaded
                    onPicked: function (v) { daemon.setConfig({ tone_style: v }) }
                }
            }

            Divider {}
            MenuRow {
                id: nfcRow
                text: "Chuẩn hoá clipboard"
                hint: ""
                onActivated: {
                    var t = Quickshell.clipboardText
                    if (!t) { hint = "Clipboard trống"; return }
                    var nfc = t.normalize("NFC")
                    if (nfc === t) { hint = "Đã chuẩn sẵn"; return }
                    Quickshell.clipboardText = nfc
                    hint = "Đã chuẩn hoá"
                }
            }
            MenuRow {
                text: "Cấu hình…"
                onActivated: {
                    Quickshell.execDetached([Quickshell.env("VI_SETTINGS_EXE") || "vi-settings"])
                    root.close()
                }
            }
            MenuRow {
                text: "Thoát vi-im"
                danger: true
                enabled: daemon.connected
                onActivated: { daemon.quitDaemon(); quitTimer.start() }
                Timer { id: quitTimer; interval: 120; onTriggered: root.close() }
            }
        }
    }

    component Divider: Rectangle {
        Layout.fillWidth: true
        Layout.topMargin: 4
        Layout.bottomMargin: 4
        Layout.leftMargin: 6
        Layout.rightMargin: 6
        implicitHeight: 1
        color: Theme.line
    }

    component Caption: Text {
        Layout.leftMargin: 12
        Layout.topMargin: 4
        Layout.bottomMargin: 2
        color: Theme.faint
        font.pixelSize: Theme.fsSmall
    }

    // A label with a control on the right (no hover, not a menu action).
    component OptionRow: Item {
        id: opt
        property string text
        default property alias control: optSlot.data
        Layout.fillWidth: true
        implicitHeight: 40
        Text {
            anchors { left: parent.left; leftMargin: 12; verticalCenter: parent.verticalCenter }
            text: opt.text
            color: Theme.ink
            font.pixelSize: Theme.fsBody
        }
        Item {
            id: optSlot
            anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
            width: childrenRect.width
            height: childrenRect.height
        }
    }

    // Menu action / radio row. Up/Down walk the rows, Enter/Space activate.
    component MenuRow: Rectangle {
        id: mr
        property string text
        property string hint
        property bool checkable: false
        property bool checked: false
        property bool danger: false
        signal activated

        Layout.fillWidth: true
        implicitHeight: 34
        radius: 6
        color: (mrMa.containsMouse || activeFocus) && enabled ? Theme.hover : "transparent"
        opacity: enabled ? 1 : 0.45
        activeFocusOnTab: true
        Accessible.role: checkable ? Accessible.RadioButton : Accessible.MenuItem
        Accessible.name: text
        Accessible.checked: checked
        Keys.onReturnPressed: if (enabled) activated()
        Keys.onSpacePressed: if (enabled) activated()
        Keys.onUpPressed: nextItemInFocusChain(false).forceActiveFocus()
        Keys.onDownPressed: nextItemInFocusChain(true).forceActiveFocus()
        Keys.onEscapePressed: root.close()

        Text {   // radio mark, only for checkable rows
            id: mark
            anchors { left: parent.left; leftMargin: 12; verticalCenter: parent.verticalCenter }
            width: mr.checkable ? 16 : 0
            text: mr.checked ? "✓" : ""
            color: Theme.accent
            font { pixelSize: Theme.fsBody; weight: Font.DemiBold }
        }
        Text {
            anchors { left: mark.right; leftMargin: mr.checkable ? 6 : 0; verticalCenter: parent.verticalCenter }
            text: mr.text
            color: mr.danger ? Theme.off : Theme.ink
            font { pixelSize: Theme.fsBody; weight: mr.checked ? Font.DemiBold : Font.Normal }
        }
        Text {
            anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
            text: mr.hint
            color: Theme.faint
            font.pixelSize: Theme.fsSmall
        }
        MouseArea {
            id: mrMa
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: if (mr.enabled) mr.activated()
        }
    }
}
