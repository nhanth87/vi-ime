# CALL FLOW — Toàn bộ đường đi của 1 keystroke

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                     USER GÕ PHÍM (bàn phím vật lý)                          │
└────────────────────────────────────┬────────────────────────────────────────┘
                                     │
                    ┌────────────────┴────────────────┐
                    │   main.rs: main()               │
                    │   DaemonEvent channel           │
                    └────────────────┬────────────────┘
                                     │
              ┌──────────────────────┴──────────────────────┐
              │  Compositor báo FocusEvent { app_id }       │
              │  compositor/niri.rs                         │
              │  compositor/wlr_toplevel.rs                 │
              └──────────────────────┬──────────────────────┘
                                     │
           ┌─────────────────────────┴─────────────────────────┐
           │              LAYER 0: PATH SELECTION               │
           │          legacy_grab.rs                            │
           │                                                    │
           │  app_id ─┬─ is_legacy_app()?                       │
           │          │   LEAGCY_APP_PREFIXES:                  │
           │          │   "libreoffice", "soffice", "onlyoffice" │
           │          │   → YES: evdev path ngay lập tức        │
           │          │                                         │
           │          ├─ is_xwayland_fallback_app()?            │
           │          │   XWAYLAND_FALLBACK_PREFIXES:           │
           │          │   "google-chrome", "chromium", "brave"… │
           │          │   → YES: probe 2s, ko Activate → evdev  │
           │          │                                         │
           │          └─ (default): WAYLAND PATH                │
           └─────────────────────────┬─────────────────────────┘
                                     │
              ┌──────────────────────┴──────────────────────┐
              │                  PATH SPLIT                  │
              └───────┬──────────────────────────┬──────────┘
                      │                          │
           ┌──────────▼──────────┐    ┌──────────▼──────────┐
           │   WAYLAND PATH      │    │    EVDEV PATH        │
           │   (Firefox, term,   │    │   (LO, OO, Chrome)   │
           │    VS Code, ...)    │    │                      │
           └──────────┬──────────┘    └──────────┬──────────┘
                      │                          │
                      │                          │
    ┌─────────────────▼────────────────┐  ┌──────▼──────────────────────┐
    │ wayland/dispatch.rs:Activate    │  │ evdev_mode.rs:run_scoped()   │
    │                                 │  │                              │
    │ ├─ maybe_reconfigure()          │  │ ├─ LegacyGrab::start()       │
    │ │  ├─ state.rs:361              │  │ │  ├─ check enabled           │
    │ │  │  ├─ set_app_context()      │  │ │  ├─ set ActivePath::Evdev   │
    │ │  │  │  → CHEAT ENGINE có app  │  │ │  ├─ spawn reader_thread     │
    │ │  │  ├─ check_rearm_timeout()  │  │ │  │  evdev_mode.rs:reader_loop│
    │ │  │  │  → auto-detect 1-shot   │  │ │  │  poll()+fetch_events()   │
    │ │  │  └─ plugin.on_focus_change│  │ │  │  → mpsc unbounded channel│
    │ │  └─ actions.rs:145           │  │ │  └─ spawn consumer_thread   │
    │ │     └─ check ActivePath       │  │ │     evdev_mode.rs:           │
    │ │        → skip if Evdev owns   │  │ │     consumer_loop()         │
    │ └─ process_key()               │  │ │                              │
    └────────────────┬────────────────┘  │ ├─ handle_key()              │
                     │                   │ │  → evdev_compose.rs        │
                     │                   │ │     NonPreeditEngine       │
                     │                   │ │     .push_key(char)        │
                     │                   │ └─ backspace_then_type()     │
                     │                   │    → LAYER 1: TYPER          │
                     │                   └────────────────┬─────────────┘
                     │                                    │
                     │                    ┌───────────────▼──────────────┐
                     │                    │  LAYER 1: TYPER SELECTION    │
                     │                    │  legacy_grab.rs:86           │
                     │                    │  evdev_inject.rs             │
                     │                    │                              │
                     │                    │  needs_injector_typer()?     │
                     │                    │   ├─ OnlyOffice → xdotool    │
                     │                    │   │  Injector::backspace_    │
                     │                    │   │  then_type()             │
                     │                    │   │  settle pacing R19       │
                     │                    │   └─ Khác → EvdevTyper       │
                     │                    │      native vk keyboard      │
                     │                    └───────────────┬──────────────┘
                     │                                    │
                     │                    ┌───────────────▼──────────────┐
                     │                    │  LAYER 2: TIMING PROFILE     │
                     │                    │  client_profile.rs           │
                     │                    │                              │
                     │                    │  ClientProfile::detect()     │
                     │                    │   ├─ libreoffice → 20/20/30  │
                     │                    │   ├─ onlyoffice → 20/20/30  │
                     │                    │   ├─ chromium   → 8/10/20   │
                     │                    │   ├─ firefox    → 5/5/15    │
                     │                    │   └─ terminal   → 3/3/5     │
                     │                    │                              │
                     │                    │  backspace_then_type():      │
                     │                    │   ├─ BS: profile.bs_delay_ms │
                     │                    │   ├─ glyph: profile.glyph_ms │
                     │                    │   ├─ pre-glyph settle        │
                     │                    │   └─ batch_safe? → burst     │
                     │                    └───────────────┬──────────────┘
                     │                                    │
                     └────────────┬───────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │         ENGINE CORE                    │
                     │    engine/fast_engine.rs               │
                     │    NonPreeditEngine::push_key(char)    │
                     └────────────┬──────────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │  LAYER 3: CHEAT / WORD OVERRIDE       │
                     │  engine/cheat.rs                      │
                     │                                       │
                     │  smart_commit_english_only()          │
                     │   ├─ cheat::should_force_english()    │
                     │   │   ├─ CHEATS (built-in 120+ words) │
                     │   │   └─ RUNTIME_CHEATS (hot-reload)  │
                     │   │                                   │
                     │   │   MATCH:                          │
                     │   │   "warp"→skip compose             │
                     │   │   "browser"→skip compose          │
                     │   │   "draw"→skip compose             │
                     │   │   log: [CHEAT] forced English     │
                     │   │                                   │
                     │   └─ NO MATCH → continue              │
                     └────────────┬──────────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │    Engine::push_key(char)              │
                     │    engine/engine.rs                    │
                     │                                       │
                     │    syllable::process(raw_keys)          │
                     │    engine/syllable.rs                  │
                     │    ├─ NFD decompose                    │
                     │    ├─ Unicode math (R14)               │
                     │    ├─ tone placement algorithm         │
                     │    └─ NFC compose → display            │
                     └────────────┬──────────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │  LAYER 4: ENGLISH RESTORE (R9)        │
                     │  engine/engine.rs:smart_commit_*()    │
                     │  engine/viet_dict.rs                  │
                     │                                       │
                     │  ├─ is_english_word(raw_keys)?         │
                     │  │   data/english_common.txt (1070 từ)│
                     │  │   "test"→test, "user"→user         │
                     │  │                                    │
                     │  └─ is_viet_syllable(render)?          │
                     │      data/viet_syllables.txt           │
                     │      "cuar"→"của"✓, "test"→"tét"✗   │
                     └────────────┬──────────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │           OUTPUT                       │
                     │                                       │
                     │  NonPreeditAction {                   │
                     │    CommitWithBackspace {              │
                     │      text: "của",                     │
                     │      backspace_count: 4               │
                     │    }                                  │
                     │  }                                    │
                     └────────────┬──────────────────────────┘
                                  │
                     ┌────────────▼──────────────────────────┐
                     │  LAYER 5: SEND TO APP                 │
                     │                                       │
                     │  Wayland path:                        │
                     │   actions.rs:sync_shown()             │
                     │   ├─ diff(old, new) → bs + suffix     │
                     │   ├─ VietTyper.backspace_then_type()  │
                     │   │  wayland/viet_typer.rs            │
                     │   │  roundtrip() mỗi BS               │
                     │   └─ live_echo_pending counter        │
                     │                                       │
                     │  Evdev path:                          │
                     │   evdev_typer.rs                      │
                     │   hoặc evdev_inject.rs (xdotool)     │
                     │   ├─ roundtrip() / flush()            │
                     │   └─ sleep(profile.glyph_delay_ms)    │
                     └───────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════
                           COMPLETE FILE MAP
═══════════════════════════════════════════════════════════════════════════

main.rs
├── init_tracing()          → file log + stderr
├── ConfigManager           → setting.conf
├── event loop              → DaemonEvent channel
├── [focus change]
│   ├── compositor/niri.rs  → spawn_niri_event_stream()
│   ├── compositor/wlr_toplevel.rs → spawn_wlr_toplevel_stream()
│   └── compositor/mod.rs   → KNOWN_TERMINALS, AppCategory
│
├── [path selection]
│   └── legacy_grab.rs
│       ├── LEGACY_APP_PREFIXES      (libreoffice, soffice, onlyoffice)
│       ├── XWAYLAND_FALLBACK_PREFIXES (chrome, chromium, brave, edge, …)
│       ├── INJECTOR_TYPER_PREFIXES   (onlyoffice)
│       ├── is_legacy_app()
│       ├── is_xwayland_fallback_app()
│       └── needs_injector_typer()
│
├── WAYLAND PATH ─────────────────────────────────────────────
│   └── wayland/
│       ├── mod.rs              → event loop + poll_timeout
│       ├── dispatch.rs         → Activate, Deactivate, Done, process_key
│       ├── state.rs
│       │   ├── ImeAppState     → engine, current_app_id, re-arm detection
│       │   ├── maybe_reconfigure()
│       │   │   ├── set_app_context() → CHEAT ENGINE
│       │   │   ├── check_rearm_timeout() → auto-detect 1-shot
│       │   │   └── plugin.on_focus_change()
│       │   └── live_echo_pending counter (R23)
│       ├── actions.rs
│       │   ├── process_key()   → check ActivePath atomic
│       │   └── sync_shown()    → diff + backspace_then_type
│       ├── runtime.rs
│       │   ├── RuntimeConfig   → snapshot, generation
│       │   └── ActivePath      → Wayland/Evdev/None atomic
│       ├── viet_typer.rs       → VietTyper (Wayland vk, conn riêng R21)
│       └── feedback.rs         → ImeFeedback::OneShotDetected
│
├── EVDEV PATH ───────────────────────────────────────────────
│   └── evdev_mode.rs
│       ├── run_scoped()
│       ├── reader_loop()        → poll() + fetch_events() (R20)
│       ├── consumer_loop()      → handle_key() → typer
│       └── mpsc unbounded channel
│   └── legacy_grab.rs
│       └── LegacyGrab::start()
│           ├── set ActivePath::Evdev
│           └── spawn evdev thread
│   └── evdev_compose.rs
│       ├── Composer
│       └── NonPreeditEngine.push_key()
│   └── evdev_typer.rs
│       ├── EvdevTyper::new(profile)
│       └── backspace_then_type()
│           ├── batch_safe? → burst BS
│           └── profile delays
│   └── evdev_inject.rs
│       ├── Typer::detect()     → xdotool vs native vk
│       └── Injector::backspace_then_type()  → OnlyOffice
│
└── ENGINE CORE ─────────────────────────────────────────────
    └── engine/
        ├── mod.rs              → exports
        ├── fast_engine.rs      → NonPreeditEngine
        │   ├── push_key(char)  → Action
        │   ├── set_app_context()
        │   └── smart_commit_english_only(app_id)
        ├── engine.rs           → Engine
        │   ├── push_key()      → syllable::process()
        │   ├── smart_commit_english_only(app_id)
        │   │   ├── cheat::should_force_english()  ★
        │   │   ├── is_english_word()
        │   │   └── is_viet_syllable()
        │   └── backspace()     → undo stack
        ├── cheat.rs            → CHEATS + RUNTIME_CHEATS ★
        │   ├── should_force_english(app_id, raw_keys)
        │   ├── add_runtime_rule(app, word)
        │   └── 120+ built-in rules
        ├── syllable.rs         → NFD/Unicode math engine
        ├── glyph.rs            → char inventory, base_of()
        ├── viet_dict.rs        → is_english_word(), is_viet_syllable()
        ├── tone.rs             → Tone enum
        ├── emoji.rs            → emoji expansion
        └── data/
            ├── english_common.txt   (1070 words)
            └── viet_syllables.txt   (~4800 syllables)

    └── client_profile.rs       → ClientProfile::detect() ★
        ├── XWAYLAND_BROWSERS
        ├── NATIVE_BROWSERS
        └── KNOWN_TERMINALS (via compositor)


═══════════════════════════════════════════════════════════════════════════
                        KEY DATA FLOW (sequence)
═══════════════════════════════════════════════════════════════════════════

1. COMPOSITOR → main.rs
   FocusEvent { app_id: "chromium-browser" }

2. main.rs → legacy_grab.rs
   is_legacy_app("chromium-browser")? → NO
   is_xwayland_fallback_app("chromium-browser")? → YES
   → probe timeout 2s → no Activate → spawn LegacyGrab

3. legacy_grab.rs → evdev_mode.rs
   run_scoped(ClientProfile::detect("chromium-browser"))
   → profile: { bs:8ms, glyph:10ms, batch_safe:true }

4. evdev_mode.rs (reader_loop)
   poll() → fetch_events() → push (KeyCode, i32) vào mpsc channel

5. evdev_mode.rs (consumer_loop)
   recv() → keycode → evdev_compose → NonPreeditEngine.push_key(char)

6. engine/fast_engine.rs
   set_app_context("chromium-browser")  ← từ maybe_reconfigure
   push_key('w') → raw_keys: ['w']
   push_key('a') → raw_keys: ['w','a']
   push_key('r') → raw_keys: ['w','a','r']
   push_key('p') → raw_keys: ['w','a','r','p']

7. engine/engine.rs (word boundary = space)
   smart_commit_english_only(Some("chromium-browser"))
   ├─ cheat::should_force_english("chromium-browser", ['w','a','r','p'])
   │  └─ match CheatRule("*", "warp") → YES!
   │     log: [CHEAT] app="chromium-browser" word="warp" forced English
   │     return "warp"  ← raw keys as-is
   └─ skip compose, skip dictionary check

8. evdev_compose.rs → evdev_typer.rs
   backspace_then_type(bs=0, text="warp")
   ├─ No backspaces → skip BS phase
   ├─ For each glyph:
   │   ├─ tap(glyph)
   │   ├─ flush()
   │   └─ sleep(10ms)  ← profile.glyph_delay_ms
   └─ Done → "warp" appears on screen

───────────────────────────────────────────────────────────────────────────

Same flow for a VIETNAMESE word "của":

5. push_key('c') → raw: ['c']
   push_key('u') → raw: ['c','u']
   push_key('a') → raw: ['c','u','a']
   push_key('r') → raw: ['c','u','a','r']

7. cheat::should_force_english("chromium", ['c','u','a','r'])
   → NO MATCH ("cuar" is not in CHEATS)

7. syllable::process(['c','u','a','r'])
   → onset="c", nucleus="ua", coda="r", tone=hỏi
   → display="của", last_valid=true

7. smart_commit_english_only()
   method=Telex → skip English restore
   return "của"

8. evdev_typer::backspace_then_type(bs=0, text="của")
   For 'c': tap(26), flush(), sleep(10ms)    ← keycode from static keymap
   For 'ủ': tap(17|SHIFT), flush(), sleep(10ms)
   For 'a': tap(38), flush(), sleep(10ms)
   → "của" renders correctly

