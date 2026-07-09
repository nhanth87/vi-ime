// SPDX-License-Identifier: GPL-3.0-or-later OR Commercial
// Copyright (c) 2024-2026 vi-im contributors
// main.qml — vi-settings: ONE small, self-contained, centered window.
//
// Self-contained on purpose: a single file, no qrc pages, no separate IPC
// singleton. Talks to vi-daemon over its unix socket via Quickshell.Io.Socket
// (the machine has Quickshell.Io, NOT Quickshell.Ipc). The socket lives at
// $XDG_RUNTIME_DIR/vi-ime/ipc.sock — the exact path the daemon binds (ipc.rs).
// Title is kept stable ("vi-im Settings") so the Rust launcher can float +
// center it over compositor IPC.

import Quickshell
import Quickshell.Io
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

FloatingWindow {
    id: win
    title: "vi-im Settings"
    objectName: "vi-im-settings"
    implicitWidth: 540
    implicitHeight: 420
    minimumSize: Qt.size(480, 420)
    maximumSize: Qt.size(720, 620)
    visible: true
    color: "#1a1b26"
    
    // Thêm bo góc viền nhẹ cho toàn bộ cửa sổ (tùy thuộc vào compositor hỗ trợ)
    // Nếu dùng compositor có bo góc tự động, bạn có thể bỏ qua.

    // ── Reactive daemon state ─────────────────────────────────────
    property string inputMethod: "Telex"
    property string toneStyle:   "classic"
    property string imeMode:     "Hybrid"
    property bool   enabled:     true
    property var    appList:     []
    property bool   connected:   false

    // ── Daemon IPC over the unix socket ───────────────────────────
    property var _pending: []

    function send(obj, cb) {
        if (!sock.connected) return
        if (cb) _pending.push(cb)
        sock.write(JSON.stringify(obj) + "\n")
    }
    function setConfig(u)      { send(Object.assign({ cmd: "set_config" }, u),
                                      function (r) { if (r && r.reload) refreshState() }) }
    function addApp(id, m)     { send({ cmd: "add_app", app_id: id, method: m || "", ime_mode: "" },
                                      function (r) { if (r && r.reload) refreshApps() }) }
    function removeApp(id)     { send({ cmd: "remove_app", app_id: id },
                                      function (r) { if (r && r.reload) refreshApps() }) }
    function refreshState()    { send({ cmd: "get_config" }, function (r) {
        if (!r || r.error) return
        if (r.input_method) win.inputMethod = r.input_method
        if (r.tone_style)   win.toneStyle   = r.tone_style
        if (r.ime_mode)     win.imeMode     = r.ime_mode
        if (r.enabled !== undefined) win.enabled = r.enabled
    }) }
    function refreshApps()     { send({ cmd: "list_apps" }, function (r) {
        if (r && !r.error) win.appList = r.apps || []
    }) }

    Socket {
        id: sock
        path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/vi-ime/ipc.sock"
        connected: true
        parser: SplitParser {
            splitMarker: "\n"
            onRead: function (line) {
                var resp
                try { resp = JSON.parse(line) } catch (e) { return }
                if (win._pending.length > 0) {
                    var cb = win._pending.shift()
                    try { cb(resp) } catch (e) {}
                }
            }
        }
        onConnectedChanged: {
            win.connected = connected
            if (connected) { win.refreshState(); win.refreshApps() }
        }
    }

    // Đồng bộ định kỳ: đổi config từ TRAY/CLI khi cửa sổ đang mở cũng
    // nhảy theo trong GUI (2s một lần, chỉ khi còn kết nối).
    Timer {
        interval: 2000; repeat: true; running: win.connected
        onTriggered: win.refreshState()
    }

    // ── Small reusable pill button ────────────────────────────────
    // LƯU Ý: màu phải là BINDING thuần (không gán trong onEntered/onExited —
    // gán trực tiếp sẽ cắt đứt binding `active`, dấu chọn không bao giờ
    // nhảy theo config nữa: chính là bug "bấm mà không đổi màu").
    component Pill: Rectangle {
        property string label
        property bool active
        property bool showCheck: true
        signal clicked
        implicitWidth: t.implicitWidth + 26
        implicitHeight: 34
        radius: 8
        color: ma.containsMouse
               ? (active ? "#7aa2f755" : "#3b4261")
               : (active ? "#7aa2f733" : "#292e42")
        border { color: active ? "#7aa2f7" : "#3b4261"; width: 1 }

        Text {
            id: t; anchors.centerIn: parent
            text: (parent.active && parent.showCheck ? "✓ " : "") + parent.label
            color: parent.active ? "#7aa2f7" : "#c0caf5"
            font { pixelSize: 12; bold: parent.active }
        }
        MouseArea {
            id: ma
            anchors.fill: parent; cursorShape: Qt.PointingHandCursor
            hoverEnabled: true
            onClicked: parent.clicked()
        }
    }
    
    component Heading: Text {
        color: "#7aa2f7"
        font { pixelSize: 13; bold: true }
        Layout.topMargin: 10
    }

    // ── Header ────────────────────────────────────────────────────
    Rectangle {
        id: header
        anchors { top: parent.top; left: parent.left; right: parent.right }
        height: 48; color: "#24283b"
        
        RowLayout {
            anchors { fill: parent; leftMargin: 16; rightMargin: 12 }
            spacing: 12
            
            Text {
                text: "vi-im"
                font { pixelSize: 18; bold: true }
                color: "#7aa2f7"
            }
            
            Item { Layout.fillWidth: true }
            
            Rectangle {
                radius: 6; color: win.connected ? "#9ece6a22" : "#f7768e22"
                implicitWidth: dot.implicitWidth + 18; implicitHeight: 26
                Text { id: dot; anchors.centerIn: parent
                    text: win.connected ? ("● " + (win.inputMethod === "Smart" ? "Tự do" : win.inputMethod)) : "● offline"
                    color: win.connected ? "#9ece6a" : "#f7768e"; font.pixelSize: 12 }
            }
            
            // ── Nút Hide / Đóng cửa sổ (Daemon vẫn chạy) ──
            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                width: 28; height: 28; radius: 6
                color: closeMouseArea.containsMouse ? "#f7768e33" : "transparent"
                
                Text { 
                    anchors.centerIn: parent; text: "✕"
                    color: closeMouseArea.containsMouse ? "#f7768e" : "#565f89"
                    font { pixelSize: 16; bold: true } 
                }
                MouseArea {
                    id: closeMouseArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        // Thoát hẳn process. Chỉ ẩn (visible=false) sẽ để
                        // quickshell sống mãi giữ single-instance lock —
                        // lần mở sau bị "already running" và không lên.
                        Qt.quit();
                    }
                }
            }
        }
    }

    TabBar {
        id: tabs
        anchors { top: header.bottom; left: parent.left; right: parent.right }
        background: Rectangle { color: "#1f2335" }
        TabButton { text: "Chung";     width: implicitWidth + 24 }
        TabButton { text: "Ứng dụng";  width: implicitWidth + 24 }
        TabButton { text: "Phím tắt";  width: implicitWidth + 24 }
    }

    StackLayout {
        anchors { top: tabs.bottom; left: parent.left; right: parent.right; bottom: parent.bottom
                  margins: 20 } // Nới margin một chút cho thanh thoát
        currentIndex: tabs.currentIndex

        // ── Tab 1: Chung ─────────────────────────────────────────
        ColumnLayout {
            spacing: 12
            Heading { text: "Kiểu gõ" }
            RowLayout { spacing: 8
                Repeater { model: [ "Telex", "VNI", "Tự do" ]
                    Pill { label: modelData; active: win.inputMethod === modelData
                           onClicked: win.setConfig({ input_method: modelData }) } }
            }
            Heading { text: "Kiểu bỏ dấu" }
            RowLayout { spacing: 8
                Pill { label: "Kiểu cũ (hòa)";  active: win.toneStyle === "classic"
                       onClicked: win.setConfig({ tone_style: "classic" }) }
                Pill { label: "Kiểu mới (hoà)"; active: win.toneStyle === "modern"
                       onClicked: win.setConfig({ tone_style: "modern" }) }
            }
            Heading { text: "Chế độ hiển thị" }
            RowLayout { spacing: 8
                Pill { label: "Preedit (gạch chân khi gõ)"
                       active: win.imeMode === "Preedit"
                       onClicked: win.setConfig({ ime_mode: "Preedit" }) }
                Pill { label: "NonPreedit (gõ thẳng, kiểu cổ điển)"
                       active: win.imeMode === "NonPreedit"
                       onClicked: win.setConfig({ ime_mode: "NonPreedit" }) }
            }
            Heading { text: "Trạng thái" }
            RowLayout { spacing: 10
                Switch {
                    // checked là BINDING theo win.enabled; user gạt thì chỉ
                    // gửi lệnh — trạng thái thật quay về qua refreshState,
                    // nên công tắc luôn khớp daemon (kể cả khi lệnh fail).
                    checked: win.enabled
                    onToggled: { win.setConfig({ enabled: checked }); checked = Qt.binding(function () { return win.enabled }) }
                }
                Text { text: win.enabled ? "🟢 Đang hoạt động" : "🔴 Đã tắt"
                       color: win.enabled ? "#9ece6a" : "#f7768e"
                       font { pixelSize: 13; bold: true }; Layout.alignment: Qt.AlignVCenter }
            }
            Item { Layout.fillHeight: true }
        }

        // ── Tab 2: Ứng dụng ──────────────────────────────────────
                // ── Tab 2: Ứng dụng ──────────────────────────────────────
        ColumnLayout {
            spacing: 16 // Tăng khoảng cách để thoáng giao diện
            
            Text { 
                text: "Cấu hình kiểu gõ riêng cho từng ứng dụng. Bỏ trống sẽ dùng thiết lập mặc định."
                color: "#a9b1d6"; font.pixelSize: 13; Layout.fillWidth: true; wrapMode: Text.WordWrap 
            }
            
            // Hàng nhập liệu: Căn bằng chiều cao và chia layout hợp lý
            RowLayout { 
                Layout.fillWidth: true; spacing: 10
                
                TextField { 
                    id: newId
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36 // Chiều cao đồng bộ
                    placeholderText: "Tên app (vd: firefox)"
                    placeholderTextColor: "#565f89"
                    color: "#c0caf5"; font.pixelSize: 13
                    verticalAlignment: TextInput.AlignVCenter // Chữ căn giữa dọc
                    background: Rectangle { 
                        radius: 6; color: "#292e42"
                        border { color: newId.activeFocus ? "#7aa2f7" : "#3b4261" } 
                    } 
                }
                
                ComboBox { 
                    id: newMethod
                    model: [ "Mặc định", "Telex", "VNI", "Tự do" ]
                    Layout.preferredWidth: 110
                    Layout.preferredHeight: 36
                    contentItem: Text { 
                        text: newMethod.currentText; color: "#9ece6a"
                        font.pixelSize: 13; leftPadding: 10; 
                        verticalAlignment: Text.AlignVCenter 
                    }
                    background: Rectangle {
                        radius: 6; color: "#292e42"
                        border { color: "#3b4261" }
                    }
                }
                
                Pill { 
                    label: "+ Thêm"
                    active: true
                    implicitHeight: 36 // Đồng bộ với Textfield và Combobox
                    onClicked: { 
                        var id = newId.text.trim(); if (id === "") return
                        var m = newMethod.currentText; if (m === "Mặc định") m = ""
                        win.addApp(id, m); newId.text = "" 
                    } 
                }
            }
            
            ListView { 
                id: apps
                Layout.fillWidth: true; Layout.fillHeight: true
                clip: true; model: win.appList; spacing: 6
                
                delegate: Rectangle {
                    width: ListView.view.width; height: 54
                    color: index % 2 ? "#1f2335" : "#1a1b26" // Màu nền xen kẽ mịn hơn
                    radius: 8
                    border.color: "#24283b"; border.width: 1
                    
                    RowLayout {
                        anchors { fill: parent; leftMargin: 16; rightMargin: 12 }
                        spacing: 12
                        
                        Text { 
                            text: modelData.icon || "📱"
                            font.pixelSize: 18 
                            Layout.alignment: Qt.AlignVCenter
                            Layout.preferredWidth: 26 // Cố định chiều rộng để text bên cạnh thẳng hàng
                            horizontalAlignment: Text.AlignHCenter
                        }
                        
                        ColumnLayout { 
                            spacing: 2; Layout.fillWidth: true
                            Layout.alignment: Qt.AlignVCenter
                            Text { 
                                text: modelData.app_name || modelData.app_id
                                color: "#c0caf5"; font.pixelSize: 14; font.bold: true 
                            }
                            Text { 
                                text: modelData.app_id; 
                                color: "#565f89"; font.pixelSize: 11 
                            } 
                        }
                        
                        // Badge (Thẻ) hiển thị kiểu gõ cho đẹp
                        Rectangle {
                            Layout.alignment: Qt.AlignVCenter
                            color: "#9ece6a1a" // Nền xanh mờ
                            border.color: "#9ece6a4d"
                            border.width: 1
                            radius: 6
                            implicitWidth: methodText.implicitWidth + 16
                            implicitHeight: 26
                            Text { 
                                id: methodText
                                anchors.centerIn: parent
                                text: modelData.method || "Mặc định"
                                color: "#9ece6a"; font.pixelSize: 12; font.bold: true
                            }
                        }
                        
                        // Nút Xóa
                        Rectangle {
                            Layout.alignment: Qt.AlignVCenter
                            width: 28; height: 28; radius: 6; color: "transparent"
                            Text { anchors.centerIn: parent; text: "✕"; color: "#f7768e"; font.pixelSize: 14 }
                            MouseArea { 
                                anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                onEntered: parent.color = "#f7768e22"
                                onExited: parent.color = "transparent"
                                onClicked: win.removeApp(modelData.app_id) 
                            }
                        }
                    }
                }
            }
        }


        // ── Tab 3: Phím tắt ──────────────────────────────────────
        ColumnLayout {
            spacing: 12
            Text { text: "Phím tắt điều khiển nhanh cho vi-im."; color: "#a9b1d6"
                   font.pixelSize: 13; Layout.fillWidth: true }
            Repeater {
                model: [
                    { label: "Bật/Tắt Game Mode", key: "Ctrl+Shift+G", desc: "Passthrough phím thô khi chơi game", live: true },
                    { label: "Đổi kiểu gõ (Telex ↔ VNI)", key: "vi-daemon --switch", desc: "Hiện chạy qua CLI (chưa gắn hotkey)", live: false },
                    { label: "Bật/Tắt IME", key: "vi-daemon --toggle", desc: "Hiện chạy qua CLI (chưa gắn hotkey)", live: false }
                ]
                delegate: Rectangle {
                    Layout.fillWidth: true; height: 64; radius: 8
                    color: "#292e42"; border { color: "#3b4261"; width: 1 }
                    RowLayout {
                        anchors { fill: parent; leftMargin: 16; rightMargin: 16 }
                        spacing: 12
                        ColumnLayout { Layout.fillWidth: true; spacing: 4
                            Text { text: modelData.label; color: "#c0caf5"; font { pixelSize: 14; bold: true } }
                            Text { text: modelData.desc; color: "#565f89"; font.pixelSize: 12 } }
                        Rectangle { radius: 6; color: "#1a1b26"
                            implicitWidth: kt.implicitWidth + 24; implicitHeight: 30
                            Text { id: kt; anchors.centerIn: parent; text: modelData.key
                                   color: modelData.live ? "#9ece6a" : "#e0af68"
                                   font { pixelSize: 12; bold: true } } }
                    }
                }
            }
            Item { Layout.fillHeight: true }
        }
    }
}
