// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Window 2.15
import Qt.labs.platform 1.1

Window {
    id: root
    title: "vi-ime"
    width: 220; height: 64
    flags: Qt.WindowStaysOnTopHint | Qt.FramelessWindowHint
    visible: true
    color: "transparent"

    property string method: "?"
    property bool enabled: true
    property string configPath: ""

    Component.onCompleted: {
        // Find config path
        var local = StandardPaths.writableLocation(StandardPaths.HomeLocation) + "/.config/vi-ime/setting.conf";
        configPath = local;
        refresh();
    }

    Timer {
        interval: 1000; running: true; repeat: true
        onTriggered: refresh()
    }

    function refresh() {
        var xhr = new XMLHttpRequest();
        xhr.open("GET", "file://" + configPath, false);
        try { xhr.send(); } catch(e) { return; }
        var txt = xhr.responseText;
        var m = txt.match(/input_method\s*=\s*"(\w+)"/);
        if (m) root.method = m[1];
        var e = txt.match(/^\s*enabled\s*=\s*(true|false)/m);
        if (e) root.enabled = e[1] === "true";
    }

    function runCmd(arg) {
        var proc = procFactory.createObject(root, {"arg": arg});
        if (proc) proc.start();
    }

    QtObject {
        id: procFactory
        property string arg

        function createObject(parent, props) {
            var p = Qt.createQmlObject(
                'import Qt.labs.platform 1.1; Process {}', parent, "proc");
            if (p) {
                p.executable = daemonPath();
                p.arguments = [props.arg];
            }
            return p;
        }
    }

    function daemonPath() {
        var e = StandardPaths.findExecutable("vi-daemon");
        if (e) return e;
        return "/usr/local/bin/vi-daemon";
    }

    Rectangle {
        anchors.fill: parent
        radius: 10
        color: "#1e1e2e"
        border { color: root.enabled ? "#a6e3a1" : "#f38ba8"; width: 2 }

        MouseArea {
            anchors.fill: parent
            property real lastX: 0; property real lastY: 0
            onPressed: { lastX = mouseX; lastY = mouseY }
            onPositionChanged: {
                root.x += mouseX - lastX;
                root.y += mouseY - lastY;
            }
        }

        Column {
            anchors.centerIn: parent
            spacing: 4

            Text {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.enabled
                    ? ("vi-ime · " + root.method + " · 🟢 Bật")
                    : ("vi-ime · 🔴 Tắt")
                color: "#cdd6f4"
                font.pixelSize: 12
                font.bold: true
            }

            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 6

                Rectangle {
                    width: 56; height: 22; radius: 6
                    color: maSwitch.containsMouse ? "#585b70" : "#45475a"
                    Text {
                        anchors.centerIn: parent
                        text: "Đổi"
                        color: "#cdd6f4"
                        font.pixelSize: 11
                    }
                    MouseArea {
                        id: maSwitch
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: root.runCmd("--switch")
                    }
                }

                Rectangle {
                    width: 56; height: 22; radius: 6
                    color: maToggle.containsMouse ? "#585b70" : "#45475a"
                    Text {
                        anchors.centerIn: parent
                        text: root.enabled ? "Tắt" : "Bật"
                        color: "#cdd6f4"
                        font.pixelSize: 11
                    }
                    MouseArea {
                        id: maToggle
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: root.runCmd("--toggle")
                    }
                }
            }
        }
    }
}
