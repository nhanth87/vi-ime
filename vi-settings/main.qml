// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// main.qml — vi-settings: the vi-im configuration window (fcitx5-configtool
// style: category sidebar on the left, form on the right).
//
// Every control applies immediately (no Apply button): an IME setting is only
// meaningful while you type, so the change has to be live. State comes from
// the daemon over IPC (Daemon.qml); controls are BOUND to it and only send
// requests, so changes from the tray/CLI show up here too.
// Title is kept stable ("vi-im Settings") so the Rust launcher can float +
// center it over compositor IPC.

import Quickshell
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

FloatingWindow {
    id: win
    title: "vi-im Settings"
    objectName: "vi-im-settings"
    implicitWidth: 820
    implicitHeight: 580
    minimumSize: Qt.size(720, 500)
    visible: true
    color: Theme.window

    Daemon {
        id: daemon
        onSaved: savedNote.flash()
        onFailed: function (msg) { banner.show(msg) }
    }

    property int page: 0
    readonly property var pages: [
        { title: "Kiểu gõ",    subtitle: "Cách gõ dấu và cách chữ hiện ra khi đang gõ." },
        { title: "Ứng dụng",   subtitle: "Kiểu gõ riêng cho từng ứng dụng, đè lên thiết lập chung." },
        { title: "Gõ tắt",     subtitle: "Gõ một từ khoá ngắn để chèn cả cụm từ." },
        { title: "Giới thiệu", subtitle: "Phiên bản, tệp cấu hình và lệnh điều khiển." }
    ]

    readonly property var methodModel: [
        { value: "Telex", label: "Telex" },
        { value: "VNI",   label: "VNI" },
        { value: "Tự do", label: "Tự do" }
    ]

    // Keystrokes → result for the live preview. "Tự do" deliberately mixes a
    // Telex word and a VNI word: that is the whole point of the mode.
    function sample(method, tone) {
        var hoa = tone === "modern" ? "hoà" : "hòa"
        if (method === "VNI")
            return [{ keys: "tie6ng1", word: "tiếng" }, { keys: "vie6t5", word: "việt" },
                    { keys: "hoa2", word: hoa }]
        if (method === "Tự do")
            return [{ keys: "tieengs", word: "tiếng" }, { keys: "vie6t5", word: "việt" },
                    { keys: "hoaf", word: hoa }]
        return [{ keys: "tieengs", word: "tiếng" }, { keys: "vieetj", word: "việt" },
                { keys: "hoaf", word: hoa }]
    }

    // ── Page-local building blocks ────────────────────────────────

    // Form row: label + description on the left, control on the right.
    component SettingRow: Item {
        id: row
        property string label
        property string description
        property bool last: false
        default property alias control: slot.data
        Layout.fillWidth: true
        implicitHeight: Math.max(Theme.rowHeight, texts.implicitHeight + 24)

        ColumnLayout {
            id: texts
            anchors { left: parent.left; right: slot.left; rightMargin: 24
                      verticalCenter: parent.verticalCenter }
            spacing: 2
            Text {
                text: row.label
                color: Theme.ink
                font.pixelSize: Theme.fsLabel
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
            }
            Text {
                visible: row.description !== ""
                text: row.description
                color: Theme.muted
                font.pixelSize: Theme.fsSmall
                Layout.fillWidth: true
                wrapMode: Text.WordWrap
                lineHeight: 1.15
            }
        }
        Item {
            id: slot
            anchors { right: parent.right; verticalCenter: parent.verticalCenter }
            width: childrenRect.width
            height: childrenRect.height
        }
        Rectangle {
            visible: !row.last
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: 1
            color: Theme.line
        }
    }

    component SectionTitle: Text {
        color: Theme.ink
        font { pixelSize: Theme.fsLabel; weight: Font.DemiBold }
        Layout.topMargin: 22
        Layout.bottomMargin: 2
    }

    // A pressed key, as on the keyboard.
    component Keycap: Rectangle {
        property string glyph
        property bool lit: true
        width: Math.max(22, k.implicitWidth + 12)
        height: 26
        radius: 5
        color: Theme.raised
        border { width: 1; color: Theme.line }
        opacity: lit ? 1 : 0.0
        Behavior on opacity { NumberAnimation { duration: 90 } }
        Rectangle {   // bottom edge: the key's depth
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom
                      leftMargin: 1; rightMargin: 1; bottomMargin: 1 }
            height: 2; radius: 2
            color: Theme.line
        }
        Text {
            id: k
            anchors.centerIn: parent
            anchors.verticalCenterOffset: -1
            text: parent.glyph
            color: Theme.ink
            font.pixelSize: Theme.fsBody
        }
    }

    // ── Layout: sidebar | content ─────────────────────────────────
    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ── Sidebar ───────────────────────────────────────────────
        Rectangle {
            Layout.preferredWidth: 220
            Layout.fillHeight: true
            color: Theme.sidebar

            ColumnLayout {
                anchors { fill: parent; margins: 16; topMargin: 20 }
                spacing: 0

                RowLayout {
                    spacing: 12
                    Logo { size: 36; on: daemon.imeEnabled || !daemon.connected }
                    ColumnLayout {
                        spacing: 1
                        Text { text: "vi-im"; color: Theme.ink
                               font { pixelSize: 16; weight: Font.DemiBold } }
                        RowLayout {
                            spacing: 6
                            Rectangle { width: 7; height: 7; radius: 4
                                        color: daemon.connected ? Theme.ok : Theme.off }
                            Text {
                                text: daemon.connected ? "Bộ gõ đang chạy" : "Không thấy bộ gõ"
                                color: Theme.muted; font.pixelSize: Theme.fsSmall
                            }
                        }
                    }
                }

                // Master switch: the one thing people come here to flip.
                Rectangle {
                    Layout.fillWidth: true
                    Layout.topMargin: 18
                    implicitHeight: 52
                    radius: Theme.radius + 2
                    color: Theme.surface
                    border { width: 1; color: Theme.line }
                    RowLayout {
                        anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
                        ColumnLayout {
                            spacing: 0
                            Text { text: "Gõ tiếng Việt"; color: Theme.ink; font.pixelSize: Theme.fsBody }
                            Text {
                                text: daemon.imeEnabled ? daemon.inputMethod : "Đang gõ tiếng Anh"
                                color: daemon.imeEnabled ? Theme.muted : Theme.off
                                font.pixelSize: Theme.fsSmall
                            }
                        }
                        Item { Layout.fillWidth: true }
                        VSwitch {
                            checked: daemon.imeEnabled
                            enabled: daemon.connected
                            accessibleName: "Gõ tiếng Việt"
                            onToggled: function (next) { daemon.setConfig({ enabled: next }) }
                        }
                    }
                }

                // Category navigation.
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 18
                    spacing: 2
                    Repeater {
                        model: win.pages
                        delegate: Rectangle {
                            id: nav
                            required property var modelData
                            required property int index
                            readonly property bool current: win.page === index
                            Layout.fillWidth: true
                            implicitHeight: 34
                            radius: Theme.radius
                            color: current ? Theme.accentSoft : (navMa.containsMouse ? Theme.hover : "transparent")
                            activeFocusOnTab: true
                            Accessible.role: Accessible.PageTab
                            Accessible.name: modelData.title
                            Keys.onReturnPressed: win.page = index
                            Keys.onSpacePressed: win.page = index
                            Keys.onUpPressed: nextItemInFocusChain(false).forceActiveFocus()
                            Keys.onDownPressed: nextItemInFocusChain(true).forceActiveFocus()

                            Rectangle {   // selection bar
                                visible: nav.current
                                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                                width: 3; height: 16; radius: 2
                                color: Theme.accent
                            }
                            Text {
                                anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
                                text: nav.modelData.title
                                color: nav.current ? Theme.ink : Theme.muted
                                font { pixelSize: Theme.fsBody; weight: nav.current ? Font.DemiBold : Font.Normal }
                            }
                            Rectangle {
                                anchors { fill: parent; margins: -2 }
                                radius: Theme.radius + 2
                                color: "transparent"
                                border { width: 2; color: Theme.accent }
                                visible: nav.activeFocus
                            }
                            MouseArea {
                                id: navMa
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: win.page = nav.index
                            }
                        }
                    }
                }

                Item { Layout.fillHeight: true }

                Text {
                    text: daemon.version ? "Phiên bản " + daemon.version : ""
                    color: Theme.faint
                    font.pixelSize: Theme.fsSmall
                }
            }
            Rectangle {
                anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
                width: 1
                color: Theme.line
            }
        }

        // ── Content ───────────────────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: Theme.surface

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                // Page header.
                RowLayout {
                    Layout.fillWidth: true
                    Layout.leftMargin: 32
                    Layout.rightMargin: 24
                    Layout.topMargin: 24
                    ColumnLayout {
                        spacing: 4
                        Text { text: win.pages[win.page].title; color: Theme.ink
                               font { pixelSize: Theme.fsTitle; weight: Font.DemiBold } }
                        Text { text: win.pages[win.page].subtitle; color: Theme.muted
                               font.pixelSize: Theme.fsBody }
                    }
                    Item { Layout.fillWidth: true }
                    Text {
                        id: savedNote
                        text: "Đã lưu"
                        color: Theme.ok
                        font.pixelSize: Theme.fsSmall
                        opacity: 0
                        Layout.alignment: Qt.AlignTop
                        function flash() { fade.restart() }
                        SequentialAnimation {
                            id: fade
                            NumberAnimation { target: savedNote; property: "opacity"; to: 1; duration: 80 }
                            PauseAnimation { duration: 1100 }
                            NumberAnimation { target: savedNote; property: "opacity"; to: 0; duration: 400 }
                        }
                    }
                }

                // Error / offline banner.
                Rectangle {
                    id: banner
                    property string message: ""
                    function show(m) { message = m; hideTimer.restart() }
                    readonly property bool offline: !daemon.connected
                    visible: offline || message !== ""
                    Layout.fillWidth: true
                    Layout.leftMargin: 32
                    Layout.rightMargin: 24
                    Layout.topMargin: 16
                    implicitHeight: bannerRow.implicitHeight + 20
                    radius: Theme.radius
                    color: Theme.offSoft
                    Timer { id: hideTimer; interval: 5000; onTriggered: banner.message = "" }
                    RowLayout {
                        id: bannerRow
                        anchors { fill: parent; leftMargin: 14; rightMargin: 10 }
                        Text {
                            Layout.fillWidth: true
                            wrapMode: Text.WordWrap
                            color: Theme.ink
                            font.pixelSize: Theme.fsBody
                            text: banner.offline
                                  ? "Không kết nối được vi-ime. Hãy chạy vi-ime (hoặc AppImage) rồi bấm Kết nối lại."
                                  : banner.message
                        }
                        VButton {
                            visible: banner.offline
                            text: "Kết nối lại"
                            onClicked: daemon.reconnect()
                        }
                    }
                }

                StackLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.topMargin: 8
                    currentIndex: win.page
                    enabled: daemon.loaded

                    // ── 1. Kiểu gõ ─────────────────────────────────
                    Flickable {
                        contentHeight: typingCol.implicitHeight + 32
                        clip: true
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.vertical: ScrollBar {}

                        ColumnLayout {
                            id: typingCol
                            width: parent.width - 56
                            x: 32
                            spacing: 0

                            // Live preview: what your keystrokes become.
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.topMargin: 12
                                implicitHeight: previewCol.implicitHeight + 28
                                radius: Theme.radius + 2
                                color: Theme.field
                                border { width: 1; color: Theme.line }

                                ColumnLayout {
                                    id: previewCol
                                    anchors { fill: parent; margins: 14; leftMargin: 18 }
                                    spacing: 10
                                    Repeater {
                                        id: samples
                                        model: win.sample(daemon.inputMethod, daemon.toneStyle)
                                        delegate: RowLayout {
                                            id: sampleRow
                                            required property var modelData
                                            required property int index
                                            spacing: 4
                                            // Keys appear one by one after a change,
                                            // then the composed word lands.
                                            property int shown: modelData.keys.length
                                            Component.onCompleted: typeIn.restart()
                                            NumberAnimation on shown {
                                                id: typeIn
                                                running: false
                                                from: 0; to: sampleRow.modelData.keys.length
                                                duration: 55 * sampleRow.modelData.keys.length + sampleRow.index * 90
                                            }
                                            Repeater {
                                                model: sampleRow.modelData.keys.split("")
                                                delegate: Keycap {
                                                    required property string modelData
                                                    required property int index
                                                    glyph: modelData
                                                    lit: index < sampleRow.shown
                                                }
                                            }
                                            Text {
                                                text: "→"
                                                color: Theme.faint
                                                font.pixelSize: Theme.fsLabel
                                                Layout.leftMargin: 8
                                                Layout.rightMargin: 8
                                            }
                                            Text {
                                                text: sampleRow.modelData.word
                                                color: Theme.ink
                                                font { pixelSize: 18; weight: Font.DemiBold }
                                                opacity: sampleRow.shown >= sampleRow.modelData.keys.length ? 1 : 0
                                                Behavior on opacity { NumberAnimation { duration: 120 } }
                                            }
                                        }
                                    }
                                }
                            }

                            SectionTitle { text: "Bộ gõ" }
                            SettingRow {
                                label: "Kiểu gõ"
                                description: daemon.inputMethod === "VNI"
                                             ? "Dùng phím số: 6 cho mũ, 7 cho móc, 8 cho trăng, 1–5 cho dấu thanh."
                                             : daemon.inputMethod === "Tự do"
                                               ? "Nhận cả phím Telex lẫn phím số VNI, trộn được trong cùng một từ."
                                               : "Dùng chữ cái: aa → â, ow → ơ, dd → đ; s f r x j cho dấu thanh."
                                Segmented {
                                    model: win.methodModel
                                    current: daemon.inputMethod
                                    segmentWidth: 72
                                    onPicked: function (v) { daemon.setConfig({ input_method: v }) }
                                }
                            }
                            SettingRow {
                                label: "Vị trí dấu"
                                description: "Kiểu cũ đặt dấu như sách báo quen dùng; kiểu mới đặt trên nguyên âm chính."
                                Segmented {
                                    model: [{ value: "classic", label: "hòa, thúy" },
                                            { value: "modern",  label: "hoà, thuý" }]
                                    current: daemon.toneStyle
                                    onPicked: function (v) { daemon.setConfig({ tone_style: v }) }
                                }
                            }
                            SettingRow {
                                label: "Bỏ dấu tự do"
                                description: "Gõ dấu thanh ở cuối từ hay ngay sau nguyên âm đều cho cùng kết quả."
                                last: true
                                VSwitch {
                                    checked: daemon.freeTone
                                    accessibleName: "Bỏ dấu tự do"
                                    onToggled: function (next) { daemon.setConfig({ free_tone_placement: next }) }
                                }
                            }

                            SectionTitle { text: "Hiển thị" }
                            SettingRow {
                                label: "Khi đang gõ"
                                description: "Gạch chân cho thấy chữ đang ghép. Gõ thẳng chèn chữ trực tiếp như bộ gõ cổ điển, hợp với terminal và trình duyệt."
                                last: true
                                Segmented {
                                    model: [{ value: "Preedit",    label: "Gạch chân" },
                                            { value: "NonPreedit", label: "Gõ thẳng" }]
                                    current: daemon.imeMode
                                    onPicked: function (v) { daemon.setConfig({ ime_mode: v }) }
                                }
                            }

                            SectionTitle { text: "Thông minh" }
                            SettingRow {
                                label: "Tự nhận ra tiếng Anh"
                                description: "Giữ nguyên các từ như test, class, window thay vì biến thành từ có dấu."
                                VSwitch {
                                    checked: daemon.autoEnglish
                                    accessibleName: "Tự nhận ra tiếng Anh"
                                    onToggled: function (next) { daemon.setConfig({ auto_detect_lang: next }) }
                                }
                            }
                            SettingRow {
                                label: "Biểu tượng cảm xúc"
                                description: "Gõ :) hoặc :smile: rồi dấu cách để ra 🙂."
                                VSwitch {
                                    checked: daemon.emoji
                                    accessibleName: "Biểu tượng cảm xúc"
                                    onToggled: function (next) { daemon.setConfig({ emoji: next }) }
                                }
                            }
                            SettingRow {
                                label: "Bảng mã"
                                description: "Dựng sẵn hợp với hầu hết ứng dụng. Chỉ chọn tổ hợp khi phần mềm yêu cầu."
                                last: true
                                VCombo {
                                    implicitWidth: 200
                                    model: [{ value: "unicodedungsan", label: "Unicode dựng sẵn" },
                                            { value: "unicodetohop",   label: "Unicode tổ hợp" }]
                                    current: daemon.outputMode
                                    onPicked: function (v) { daemon.setConfig({ output_mode: v }) }
                                }
                            }
                        }
                    }

                    // ── 2. Ứng dụng ────────────────────────────────
                    ColumnLayout {
                        spacing: 0

                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 32
                            Layout.rightMargin: 24
                            spacing: 0

                            SettingRow {
                                label: "Dùng cấu hình riêng"
                                description: "Tắt để mọi ứng dụng dùng chung thiết lập ở trang Kiểu gõ."
                                VSwitch {
                                    checked: daemon.perApp
                                    accessibleName: "Dùng cấu hình riêng"
                                    onToggled: function (next) { daemon.setConfig({ enable_per_app: next }) }
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                Layout.topMargin: 16
                                spacing: 8
                                VField {
                                    id: newApp
                                    Layout.fillWidth: true
                                    placeholderText: "Mã ứng dụng, ví dụ firefox hoặc org.libreoffice.LibreOffice"
                                    onAccepted: addAppBtn.clicked()
                                }
                                VButton {
                                    id: addAppBtn
                                    kind: "primary"
                                    text: "Thêm ứng dụng"
                                    enabled: newApp.text.trim() !== ""
                                    onClicked: {
                                        var id = newApp.text.trim()
                                        if (id === "") return
                                        daemon.setApp(id, "", "", true, null)
                                        newApp.text = ""
                                    }
                                }
                            }

                            // Column header.
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.topMargin: 18
                                visible: daemon.apps.length > 0
                                spacing: 10
                                Text { text: "Ứng dụng"; color: Theme.muted; font.pixelSize: Theme.fsSmall
                                       Layout.fillWidth: true; Layout.leftMargin: 44 }
                                Text { text: "Kiểu gõ"; color: Theme.muted; font.pixelSize: Theme.fsSmall
                                       Layout.preferredWidth: 118 }
                                Text { text: "Hiển thị"; color: Theme.muted; font.pixelSize: Theme.fsSmall
                                       Layout.preferredWidth: 118 }
                                Text { text: "Tiếng Việt"; color: Theme.muted; font.pixelSize: Theme.fsSmall
                                       Layout.preferredWidth: 64 }
                                Item { Layout.preferredWidth: 30 }
                            }
                        }

                        ListView {
                            id: appList
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Layout.leftMargin: 32
                            Layout.rightMargin: 24
                            Layout.topMargin: 6
                            clip: true
                            model: daemon.apps
                            opacity: daemon.perApp ? 1 : 0.5
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.vertical: ScrollBar {}

                            delegate: Item {
                                id: appRow
                                required property var modelData
                                width: ListView.view.width
                                height: 92
                                readonly property string methodValue:
                                    modelData.method === "Mặc định" ? "" : modelData.method
                                readonly property string modeValue:
                                    modelData.ime_mode === "Mặc định" ? "" : modelData.ime_mode
                                readonly property bool hasOverride:
                                    methodValue !== "" || modeValue !== "" || modelData.enabled === false
                                    || modelData.typing_delay_ms != null || modelData.bs_settle_ms != null
                                    || modelData.accent_settle_ms != null
                                // Write the whole row; `t` replaces one timing field.
                                function save(method, mode, on, t) {
                                    var cur = { typing: modelData.typing_delay_ms, bs: modelData.bs_settle_ms,
                                                accent: modelData.accent_settle_ms }
                                    daemon.setApp(modelData.app_id, method, mode, on, Object.assign(cur, t || {}))
                                }

                                ColumnLayout {
                                    anchors { fill: parent; topMargin: 8; bottomMargin: 8 }
                                    spacing: 6
                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 10
                                        Rectangle {   // initial-letter avatar
                                            Layout.preferredWidth: 32
                                            Layout.preferredHeight: 32
                                            radius: 16
                                            color: Theme.accentSoft
                                            Text {
                                                anchors.centerIn: parent
                                                text: appRow.modelData.app_name.replace(/^.*\./, "").charAt(0).toUpperCase()
                                                color: Theme.accent
                                                font { pixelSize: Theme.fsLabel; weight: Font.DemiBold }
                                            }
                                        }
                                        ColumnLayout {
                                            Layout.fillWidth: true
                                            spacing: 0
                                            Text {
                                                Layout.fillWidth: true
                                                text: appRow.modelData.app_name
                                                color: Theme.ink
                                                font.pixelSize: Theme.fsBody
                                                elide: Text.ElideMiddle
                                            }
                                            Text {
                                                Layout.fillWidth: true
                                                text: appRow.modelData.builtin ? "Có sẵn profile — mã " + appRow.modelData.app_id
                                                                               : appRow.modelData.app_id
                                                color: Theme.faint
                                                font.pixelSize: Theme.fsSmall
                                                elide: Text.ElideRight
                                            }
                                        }
                                        VCombo {
                                            Layout.preferredWidth: 118
                                            model: [{ value: "", label: "Theo chung" }].concat(win.methodModel)
                                            current: appRow.methodValue
                                            onPicked: function (v) {
                                                appRow.save(v, appRow.modeValue, appRow.modelData.enabled !== false)
                                            }
                                        }
                                        VCombo {
                                            Layout.preferredWidth: 118
                                            model: [{ value: "", label: "Theo chung" },
                                                    { value: "Preedit", label: "Gạch chân" },
                                                    { value: "NonPreedit", label: "Gõ thẳng" }]
                                            current: appRow.modeValue
                                            onPicked: function (v) {
                                                appRow.save(appRow.methodValue, v, appRow.modelData.enabled !== false)
                                            }
                                        }
                                        Item {
                                            Layout.preferredWidth: 64
                                            implicitHeight: 22
                                            VSwitch {
                                                checked: appRow.modelData.enabled !== false
                                                accessibleName: "Tiếng Việt trong " + appRow.modelData.app_name
                                                onToggled: function (next) {
                                                    appRow.save(appRow.methodValue, appRow.modeValue, next)
                                                }
                                            }
                                        }
                                        VButton {
                                            // Built-in rows stay listed: the button only drops the overrides.
                                            Layout.preferredWidth: appRow.modelData.builtin ? implicitWidth : 30
                                            implicitHeight: 30
                                            kind: "ghost"
                                            text: appRow.modelData.builtin ? "Đặt lại" : "✕"
                                            enabled: !appRow.modelData.builtin || appRow.hasOverride
                                            Accessible.name: (appRow.modelData.builtin ? "Đặt lại " : "Xoá ")
                                                             + appRow.modelData.app_name
                                            onClicked: daemon.removeApp(appRow.modelData.app_id)
                                        }
                                    }
                                    // Typing rhythm: empty = the built-in default (shown as placeholder).
                                    RowLayout {
                                        Layout.fillWidth: true
                                        Layout.leftMargin: 42
                                        spacing: 8
                                        Repeater {
                                            model: [
                                                { key: "typing", label: "Trễ mỗi phím",
                                                  cur: appRow.modelData.typing_delay_ms, def: appRow.modelData.default_typing_delay_ms },
                                                { key: "bs", label: "Chờ sau BackSpace",
                                                  cur: appRow.modelData.bs_settle_ms, def: appRow.modelData.default_bs_settle_ms },
                                                { key: "accent", label: "Chờ sau chữ có dấu",
                                                  cur: appRow.modelData.accent_settle_ms, def: appRow.modelData.default_accent_settle_ms }
                                            ]
                                            delegate: RowLayout {
                                                id: timing
                                                required property var modelData
                                                spacing: 4
                                                Text { text: timing.modelData.label; color: Theme.muted; font.pixelSize: Theme.fsSmall }
                                                VField {
                                                    Layout.preferredWidth: 56
                                                    implicitHeight: 26
                                                    font.pixelSize: Theme.fsSmall
                                                    horizontalAlignment: TextInput.AlignRight
                                                    validator: IntValidator { bottom: 0; top: 1000 }
                                                    text: timing.modelData.cur != null ? String(timing.modelData.cur) : ""
                                                    placeholderText: String(timing.modelData.def)
                                                    onEditingFinished: {
                                                        var v = text.trim() === "" ? null : parseInt(text)
                                                        if (v === timing.modelData.cur || (v === null && timing.modelData.cur == null)) return
                                                        var t = {}; t[timing.modelData.key] = v
                                                        appRow.save(appRow.methodValue, appRow.modeValue,
                                                                    appRow.modelData.enabled !== false, t)
                                                    }
                                                }
                                                Text { text: "ms"; color: Theme.faint; font.pixelSize: Theme.fsSmall; Layout.rightMargin: 8 }
                                            }
                                        }
                                        Item { Layout.fillWidth: true }
                                    }
                                }
                                Rectangle {
                                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                                    height: 1
                                    color: Theme.line
                                }
                            }

                            Text {
                                visible: appList.count === 0
                                anchors { top: parent.top; topMargin: 28; left: parent.left; right: parent.right }
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                                color: Theme.muted
                                font.pixelSize: Theme.fsBody
                                lineHeight: 1.3
                                text: "Chưa có ứng dụng nào được cấu hình riêng.\n"
                                      + "Mã ứng dụng là app_id hoặc class, xem bằng hyprctl clients hoặc niri msg windows."
                            }
                        }
                    }

                    // ── 3. Gõ tắt ──────────────────────────────────
                    ColumnLayout {
                        spacing: 0

                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 32
                            Layout.rightMargin: 24
                            spacing: 0

                            Text {
                                Layout.fillWidth: true
                                Layout.topMargin: 8
                                wrapMode: Text.WordWrap
                                color: Theme.muted
                                font.pixelSize: Theme.fsBody
                                text: "Gõ từ khoá rồi nhấn dấu cách, Enter hoặc dấu câu: từ khoá được thay bằng nội dung."
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.topMargin: 14
                                spacing: 8
                                VField {
                                    id: macroKey
                                    Layout.preferredWidth: 150
                                    placeholderText: "Từ khoá, ví dụ vn"
                                    onAccepted: macroValue.forceActiveFocus()
                                }
                                VField {
                                    id: macroValue
                                    Layout.fillWidth: true
                                    placeholderText: "Nội dung, ví dụ Việt Nam"
                                    onAccepted: addMacroBtn.clicked()
                                }
                                VButton {
                                    id: addMacroBtn
                                    kind: "primary"
                                    text: "Thêm"
                                    enabled: macroKey.text.trim() !== "" && macroValue.text !== ""
                                    onClicked: {
                                        if (!enabled) return
                                        daemon.setMacro(macroKey.text.trim(), macroValue.text)
                                        macroKey.text = ""; macroValue.text = ""
                                        macroKey.forceActiveFocus()
                                    }
                                }
                            }
                        }

                        ListView {
                            id: macroList
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Layout.leftMargin: 32
                            Layout.rightMargin: 24
                            Layout.topMargin: 14
                            clip: true
                            boundsBehavior: Flickable.StopAtBounds
                            ScrollBar.vertical: ScrollBar {}
                            model: Object.keys(daemon.macros).sort()

                            delegate: Item {
                                id: macroRow
                                required property string modelData
                                width: ListView.view.width
                                height: 46
                                RowLayout {
                                    anchors.fill: parent
                                    spacing: 12
                                    Item {
                                        Layout.preferredWidth: 150
                                        implicitHeight: 26
                                        Keycap { glyph: macroRow.modelData; anchors.verticalCenter: parent.verticalCenter }
                                    }
                                    Text {
                                        Layout.fillWidth: true
                                        text: daemon.macros[macroRow.modelData] || ""
                                        color: Theme.ink
                                        font.pixelSize: Theme.fsBody
                                        elide: Text.ElideRight
                                        maximumLineCount: 1
                                    }
                                    VButton {
                                        Layout.preferredWidth: 30
                                        implicitHeight: 30
                                        kind: "ghost"
                                        text: "✕"
                                        Accessible.name: "Xoá gõ tắt " + macroRow.modelData
                                        onClicked: daemon.removeMacro(macroRow.modelData)
                                    }
                                }
                                Rectangle {
                                    anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                                    height: 1
                                    color: Theme.line
                                }
                            }

                            Text {
                                visible: macroList.count === 0
                                anchors { top: parent.top; topMargin: 28; left: parent.left; right: parent.right }
                                horizontalAlignment: Text.AlignHCenter
                                color: Theme.muted
                                font.pixelSize: Theme.fsBody
                                text: "Chưa có từ gõ tắt nào. Thử thêm vn → Việt Nam."
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 32
                            Layout.rightMargin: 24
                            Layout.bottomMargin: 16
                            Layout.topMargin: 8
                            wrapMode: Text.WordWrap
                            color: Theme.faint
                            font.pixelSize: Theme.fsSmall
                            text: "Gõ tắt theo tổ hợp phím (ví dụ ctrl+alt+m) đặt trong setting.conf, mục [macro_hotkeys]."
                        }
                    }

                    // ── 4. Giới thiệu ──────────────────────────────
                    Flickable {
                        contentHeight: aboutCol.implicitHeight + 32
                        clip: true
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.vertical: ScrollBar {}

                        ColumnLayout {
                            id: aboutCol
                            width: parent.width - 56
                            x: 32
                            spacing: 0

                            RowLayout {
                                Layout.topMargin: 16
                                spacing: 16
                                Logo { size: 56 }
                                ColumnLayout {
                                    spacing: 2
                                    Text { text: "vi-im"; color: Theme.ink
                                           font { pixelSize: 22; weight: Font.DemiBold } }
                                    Text { text: "Bộ gõ tiếng Việt cho Wayland"; color: Theme.muted
                                           font.pixelSize: Theme.fsBody }
                                    Text { text: daemon.version ? "Phiên bản " + daemon.version + ", giấy phép GPL-3.0"
                                                                : "Giấy phép GPL-3.0"
                                           color: Theme.faint; font.pixelSize: Theme.fsSmall }
                                }
                            }

                            SectionTitle { text: "Tệp cấu hình" }
                            SettingRow {
                                label: daemon.configPath || "Chưa rõ"
                                description: "Lưu tự động khi bạn đổi thiết lập. Sửa tay tệp này cũng được, bộ gõ đọc lại ngay."
                                last: true
                                VButton {
                                    text: "Mở thư mục"
                                    enabled: daemon.configPath !== ""
                                    onClicked: Quickshell.execDetached(
                                        ["xdg-open", daemon.configPath.replace(/\/[^\/]*$/, "")])
                                }
                            }

                            SectionTitle { text: "Phím tắt" }
                            Repeater {
                                model: [
                                    { k: "Ctrl+Shift+G", d: "Bật hoặc tắt chế độ chơi game (gửi phím thô)" },
                                    { k: "Chuột giữa", d: "Bấm vào biểu tượng vi-im trên thanh tác vụ để mở menu nhanh" }
                                ]
                                delegate: SettingRow {
                                    required property var modelData
                                    required property int index
                                    label: modelData.d
                                    last: index === 1
                                    Keycap { glyph: modelData.k }
                                }
                            }

                            SectionTitle { text: "Dòng lệnh" }
                            Repeater {
                                model: [
                                    { k: "vi-ime --toggle", d: "Bật hoặc tắt tiếng Việt" },
                                    { k: "vi-ime --switch", d: "Chuyển kiểu gõ Telex ↔ VNI" },
                                    { k: "vi-ime --status", d: "Xem trạng thái hiện tại" },
                                    { k: "vi-ime --doctor", d: "Kiểm tra cài đặt và quyền truy cập" }
                                ]
                                delegate: SettingRow {
                                    required property var modelData
                                    required property int index
                                    label: modelData.d
                                    last: index === 3
                                    Text {
                                        text: modelData.k
                                        color: Theme.muted
                                        font { pixelSize: Theme.fsBody; family: "monospace" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
