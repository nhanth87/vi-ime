// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 Tran Huu Nhan <nhanth87>
// Daemon.qml — vi-ime IPC client shared by main.qml and menu.qml.
//
// Talks JSON lines to the daemon over $XDG_RUNTIME_DIR/vi-ime/ipc.sock (the
// exact path ipc.rs binds) via Quickshell.Io.Socket. Replies come back in
// request order, so a FIFO of callbacks pairs each reply with its request.
//
// Writes are optimistic: the matching property flips at once so the UI
// answers the click instantly, then a get_config confirms the real state.

import Quickshell
import Quickshell.Io
import QtQuick

Item {
    id: d
    visible: false

    // ── Mirrored daemon state ─────────────────────────────────────
    readonly property bool connected: sock.connected
    property bool loaded: false
    property string inputMethod: "Telex"      // "Telex" | "VNI" | "Tự do"
    property string toneStyle: "classic"      // "classic" | "modern"
    property string imeMode: "Preedit"        // "Preedit" | "NonPreedit"
    property string outputMode: "unicodedungsan"
    property bool imeEnabled: true
    property bool freeTone: true
    property bool autoEnglish: true
    property bool perApp: true
    property bool emoji: true
    property var macros: ({})
    property var apps: []
    property string configPath: ""
    property string version: ""

    // Fired after the daemon acknowledged a write / reported an error.
    signal saved()
    signal failed(string message)

    // set_config key → mirrored property, for the optimistic update.
    readonly property var _keyToProp: ({
        input_method: "inputMethod", tone_style: "toneStyle", ime_mode: "imeMode",
        output_mode: "outputMode", enabled: "imeEnabled", free_tone_placement: "freeTone",
        auto_detect_lang: "autoEnglish", enable_per_app: "perApp", emoji: "emoji"
    })
    property var _pending: []

    function send(obj, cb) {
        if (!sock.connected) { failed("Chưa kết nối được vi-ime"); return }
        _pending.push(cb || null)
        sock.write(JSON.stringify(obj) + "\n")
        sock.flush()
    }

    function _ack(r, then) {
        if (r && r.error) failed(r.error)
        else saved()
        if (then) then()
    }

    function setConfig(u) {
        for (var k in u)
            if (_keyToProp[k] !== undefined) d[_keyToProp[k]] = u[k]
        send(Object.assign({ cmd: "set_config" }, u), function (r) { _ack(r, refresh) })
    }
    function setApp(id, method, mode, on) {
        send({ cmd: "add_app", app_id: id, method: method, ime_mode: mode, enabled: on },
             function (r) { _ack(r, refreshApps) })
    }
    function removeApp(id) {
        send({ cmd: "remove_app", app_id: id }, function (r) { _ack(r, refreshApps) })
    }
    function setMacro(key, value) {
        send({ cmd: "set_macro", key: key, value: value }, function (r) { _ack(r, refresh) })
    }
    function removeMacro(key) {
        send({ cmd: "remove_macro", key: key }, function (r) { _ack(r, refresh) })
    }
    function quitDaemon() { send({ cmd: "quit" }) }

    function refresh() {
        send({ cmd: "get_config" }, function (r) {
            if (!r || r.error) return
            if (r.input_method) d.inputMethod = r.input_method
            if (r.tone_style) d.toneStyle = r.tone_style
            if (r.ime_mode) d.imeMode = r.ime_mode
            if (r.output_mode) d.outputMode = r.output_mode
            if (r.enabled !== undefined) d.imeEnabled = r.enabled
            if (r.free_tone_placement !== undefined) d.freeTone = r.free_tone_placement
            if (r.auto_detect_lang !== undefined) d.autoEnglish = r.auto_detect_lang
            if (r.enable_per_app !== undefined) d.perApp = r.enable_per_app
            if (r.emoji !== undefined) d.emoji = r.emoji
            if (r.macros) d.macros = r.macros
            if (r.config_path) d.configPath = r.config_path
            if (r.version) d.version = r.version
            d.loaded = true
        })
    }
    function refreshApps() {
        send({ cmd: "list_apps" }, function (r) { if (r && !r.error) d.apps = r.apps || [] })
    }

    function reconnect() {
        sock.connected = false
        sock.connected = true
    }

    Socket {
        id: sock
        path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/vi-ime/ipc.sock"
        connected: true
        parser: SplitParser {
            splitMarker: "\n"
            onRead: function (line) {
                var resp
                try { resp = JSON.parse(line) } catch (e) { return }
                var cb = d._pending.length > 0 ? d._pending.shift() : null
                if (cb) { try { cb(resp) } catch (e) { console.warn("vi-im ipc:", e) } }
            }
        }
        onConnectedChanged: {
            d._pending = []
            d.loaded = false
            if (connected) { d.refresh(); d.refreshApps() }
        }
    }

    // Keep in step with changes made from the tray or CLI while open, and
    // retry the socket when the daemon was restarted.
    Timer {
        interval: 2000; repeat: true; running: true
        onTriggered: sock.connected ? d.refresh() : d.reconnect()
    }
}
