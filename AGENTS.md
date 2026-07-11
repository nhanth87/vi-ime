# AGENTS.md — vi-ime AI Agent Design Contract

> **Mục đích:** Mọi AI agent khi sửa code PHẢI tuân theo file này.
> **Vi phạm:** Merge bị từ chối. Phải revert.

---

## 🗺️ Codebase Knowledge — /understand

- **Trước khi làm việc:** chạy `/understand` hoặc đọc `.understand-anything/knowledge-graph.json`.
- **Sau khi thay đổi:** chạy `/understand --full` để cập nhật knowledge graph.

---

## 🔒 DESIGN RULES — KHÔNG ĐƯỢC PHÁ VỠ

> Đọc chi tiết: [`.agents/R18-design-rules.md`](.agents/R18-design-rules.md)

**Tóm tắt 17 rules:**
- R1. Pipeline immutable (qua plugin only)
- R2. Preedit-everywhere (commit_string, KHÔNG delete_surrounding_text)
- R3. NonPreeditAction extend-only
- R4. File ≤300 dòng
- R5. 2 crate: vi-daemon + vi-settings
- R6. Godmod = debug only
- R7. commit_string cho mọi mode
- R8. Deactivate = Drop (KHÔNG commit)
- R9. English restore = validity + dictionary
- R10. Test coverage (pub fn ≥1 test)
- R11/R11b. App detection (protocol signals) + ContentType
- R12. Live reconfig (generation-gated atomics)
- R13. 4-layer config (user > learned > builtin > global)
- R14. Engine = NFD algebra, table-free
- R15. Zero-CPU idle (blocking recv only)

### R16. Focus Pipeline & Preedit-Jump Guard
> ⚠️ **ĐỌC:** [`.agents/R16-focus-pipeline.md`](.agents/R16-focus-pipeline.md)
> trước khi sửa focus, reconfigure, set_preedit, hoặc evdev fallback.

### R17. Bản đồ tính năng hay vỡ
> ⚠️ **ĐỌC:** [`.agents/R17-feature-map.md`](.agents/R17-feature-map.md)
> trước khi sửa typing path, address bar, terminal mode, hoặc click guard.

---

## 📁 File Map

> Xem chi tiết: [`.agents/file-map.md`](.agents/file-map.md)

---

## 🚫 VIOLATIONS

| Hành động | Mức | Hậu quả |
|-----------|-----|---------|
| `unwrap()` ngoài test | 🔴 | Crash |
| Đổi NonPreeditAction | 🔴 | Break dispatch |
| Bỏ Deactivate auto-commit | 🔴 | Mất chữ |
| File > 300 dòng | 🟡 | AI context |
| Circular dep | 🔴 | Không compile |
| Thiếu test | 🟡 | Regression |

---

## 🧠 Task Workflow — Supememory + Regression

> Áp dụng cho MỌI task, không ngoại lệ.

### Trước khi bắt đầu task
1. **Recall từ Supememory:** query context liên quan đến task.
2. Đọc lại Rules liên quan (đặc biệt R16/R17 nếu đụng typing/focus/preedit).

### Sau mỗi lần cập nhật code
3. **Chạy regression test:**
   ```bash
   cargo test -p vi-daemon
   # Nếu thay đổi typing path / live echo / evdev:
   ./scripts/vi-regression/run.sh
   ```
   KHÔNG được coi task hoàn thành nếu regression fail.

### Sau khi kết thúc task
4. **Update Supememory:** ghi tóm tắt task (file, rule, decision).
5. **Update `/understand`** nếu có thay đổi kiến trúc.

---

## ✅ Merge Checklist

- [ ] `cargo check` clean
- [ ] `cargo test` all pass
- [ ] No file > 300 dòng
- [ ] No `unwrap()` in production
- [ ] New code has tests
- [ ] No circular deps
- [ ] Supememory updated
- [ ] AGENTS.md updated (nếu có rule/file map thay đổi)
