// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// bar.qml — word suggestion bar (`vi-settings --bar`, started by the daemon).
//
// Subscribes to the daemon (`predict_subscribe`): one JSON line per change,
// pushed — nothing polls while idle. Shown only while there are suggestions
// (the daemon clears them a few seconds after the last keystroke). Takes no
// keyboard focus and lets clicks through: it must never steal input from
// the text field the user is typing in. Words are accepted with Alt+1..3,
// handled by the daemon.

import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import QtQuick
import QtQuick.Layouts

ShellRoot {
    id: root
    property var words: []

    Socket {
        id: sock
        path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/vi-ime/ipc.sock"
        connected: true
        parser: SplitParser {
            splitMarker: "\n"
            onRead: function (line) {
                var r
                try { r = JSON.parse(line) } catch (e) { return }
                if (r.suggestions !== undefined) root.words = r.suggestions
            }
        }
        onConnectedChanged: {
            root.words = []
            if (connected) {
                write(JSON.stringify({ cmd: "predict_subscribe" }) + "\n")
                flush()
            }
        }
    }

    // Daemon restarted: retry the socket (runs only while disconnected).
    Timer {
        interval: 2000; repeat: true
        running: !sock.connected
        onTriggered: { sock.connected = false; sock.connected = true }
    }

    PanelWindow {
        visible: root.words.length > 0
        anchors.bottom: true            // no left/right anchor → centered
        margins.bottom: 48
        implicitWidth: row.implicitWidth + 20
        implicitHeight: 34
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        mask: Region {}                 // clicks fall through to the app
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "vi-im-suggest"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

        Rectangle {
            anchors.fill: parent
            radius: Theme.radius
            color: Theme.raised
            border { width: 1; color: Theme.line }

            RowLayout {
                id: row
                anchors.centerIn: parent
                spacing: 14
                Repeater {
                    model: root.words
                    delegate: RowLayout {
                        required property string modelData
                        required property int index
                        spacing: 5
                        Text {
                            text: "Alt+" + (index + 1)
                            color: Theme.faint
                            font.pixelSize: Theme.fsSmall
                        }
                        Text {
                            text: modelData
                            color: index === 0 ? Theme.accent : Theme.ink
                            font { pixelSize: Theme.fsLabel; weight: index === 0 ? Font.DemiBold : Font.Normal }
                        }
                    }
                }
            }
        }
    }
}
