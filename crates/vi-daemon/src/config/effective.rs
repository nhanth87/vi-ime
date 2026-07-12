// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2024-2026 vi-im contributors
//! Effective-config resolution.
//!
//! Legacy 2-layer path: site > app > global (`effective_config`).
//! New 4-layer path (`effective_config_layered`, R13-layered):
//!   user site > user app > learned > builtin site > builtin app > global
//! Each resolved value carries its origin so the control panel can show
//! "bạn chỉnh" / "tự học" / "mặc định" badges.

use crate::config::builtin;
use crate::config::types::{AppConfig, ImeMode, InputMethod, OutputMode, Setting, ToneStyle};

/// Where a resolved value came from — drives the control-panel badge and
/// lets the IME thread know whether the user explicitly chose the mode
/// (ContentType-Terminal must not override an explicit user choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// setting.conf site rule (tab Website) — "bạn chỉnh".
    UserSite,
    /// setting.conf app rule (tab Ứng dụng) — "bạn chỉnh".
    UserApp,
    /// learned.toml runtime observation — "tự học".
    Learned,
    /// static builtin profile table — "mặc định".
    Builtin,
    /// global defaults — no per-app entry anywhere.
    Global,
}

impl ConfigSource {
    pub fn is_user(self) -> bool {
        matches!(self, ConfigSource::UserSite | ConfigSource::UserApp)
    }

}

/// An `EffectiveConfig` plus provenance of its `ime_mode` decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedConfig {
    pub config: EffectiveConfig,
    /// Which layer decided `ime_mode` (the adaptation-relevant field).
    pub mode_source: ConfigSource,
    /// Highest layer holding ANY entry for this app/site (badge origin).
    pub origin: ConfigSource,
}

/// Fully-resolved configuration for the currently-focused context.
/// Every field is concrete — no `Option` left to interpret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub enabled: bool,
    pub input_method: InputMethod,
    pub ime_mode: ImeMode,
    pub output_mode: OutputMode,
    pub free_tone_placement: bool,
    pub auto_detect_lang: bool,
    /// Global-only (not overridable per app/site).
    pub tone_style: ToneStyle,
    /// Emoji shortcode/emoticon expansion. Global-only.
    pub emoji: bool,
}

/// Resolve one field through the site > app > global chain.
macro_rules! resolve {
    ($site:expr, $app:expr, $global:expr, $field:ident) => {
        $site
            .and_then(|c| c.$field)
            .or_else(|| $app.and_then(|c| c.$field))
            .unwrap_or($global)
    };
}

impl Setting {
    /// Resolve the effective config for a focused window.
    ///
    /// - `app_id`: compositor app_id/class of the focused window.
    /// - `title`: window title — pass `Some` only for browsers; site rules
    ///   match by lowercase substring against it.
    ///
    /// Precedence: site override > app override > global.
    /// When `enable_per_app` is false, overrides are ignored entirely.
    pub fn effective_config(&self, app_id: Option<&str>, title: Option<&str>) -> EffectiveConfig {
        let (app_cfg, site_cfg) = if self.enable_per_app {
            let app_cfg = app_id.and_then(|id| self.app_configs.get(id));
            let site_cfg = title.and_then(|t| self.site_config_for_title(t));
            (app_cfg, site_cfg)
        } else {
            (None, None)
        };

        EffectiveConfig {
            enabled: resolve!(site_cfg, app_cfg, self.enabled, enabled),
            input_method: resolve!(site_cfg, app_cfg, self.input_method, input_method),
            ime_mode: resolve!(site_cfg, app_cfg, self.ime_mode, ime_mode),
            output_mode: resolve!(site_cfg, app_cfg, self.output_mode, output_mode),
            free_tone_placement: resolve!(site_cfg, app_cfg, self.free_tone_placement, free_tone_placement),
            auto_detect_lang: resolve!(site_cfg, app_cfg, self.auto_detect_lang, auto_detect_lang),
            tone_style: self.tone_style,
            emoji: self.emoji,
        }
    }

    /// 4-layer resolution (R13-layered). `learned` is the override derived
    /// from `LearnedStore::suggested_config(app_id)` — the daemon passes it
    /// in so vi-config stays storage-agnostic here.
    ///
    /// When `enable_per_app` is false every per-app layer is skipped and the
    /// global config applies unchanged (source = Global).
    pub fn effective_config_layered(
        &self,
        app_id: Option<&str>,
        title: Option<&str>,
        learned: Option<&AppConfig>,
    ) -> ResolvedConfig {
        if !self.enable_per_app {
            return ResolvedConfig {
                config: self.effective_config(app_id, title),
                mode_source: ConfigSource::Global,
                origin: ConfigSource::Global,
            };
        }
        let user_site = title.and_then(|t| self.site_config_for_title(t));
        let user_app = app_id.and_then(|id| self.app_configs.get(id));
        let builtin_site = title.and_then(builtin::builtin_site_profile);
        let builtin_app = app_id.and_then(builtin::builtin_app_profile);

        // Priority order, highest first.
        let layers: [(Option<&AppConfig>, ConfigSource); 5] = [
            (user_site, ConfigSource::UserSite),
            (user_app, ConfigSource::UserApp),
            (learned, ConfigSource::Learned),
            (builtin_site.as_ref(), ConfigSource::Builtin),
            (builtin_app.as_ref(), ConfigSource::Builtin),
        ];

        fn pick<T: Copy>(
            layers: &[(Option<&AppConfig>, ConfigSource)],
            global: T,
            get: fn(&AppConfig) -> Option<T>,
        ) -> (T, ConfigSource) {
            for (cfg, src) in layers {
                if let Some(v) = cfg.and_then(get) {
                    return (v, *src);
                }
            }
            (global, ConfigSource::Global)
        }

        let (enabled, _) = pick(&layers, self.enabled, |c| c.enabled);
        let (input_method, _) = pick(&layers, self.input_method, |c| c.input_method);
        let (ime_mode, mode_source) = pick(&layers, self.ime_mode, |c| c.ime_mode);
        let (output_mode, _) = pick(&layers, self.output_mode, |c| c.output_mode);
        let (free_tone_placement, _) =
            pick(&layers, self.free_tone_placement, |c| c.free_tone_placement);
        let (auto_detect_lang, _) =
            pick(&layers, self.auto_detect_lang, |c| c.auto_detect_lang);

        let origin = layers
            .iter()
            .find(|(cfg, _)| cfg.is_some())
            .map(|(_, src)| *src)
            .unwrap_or(ConfigSource::Global);

        ResolvedConfig {
            config: EffectiveConfig {
                enabled,
                input_method,
                ime_mode,
                output_mode,
                free_tone_placement,
                auto_detect_lang,
                tone_style: self.tone_style,
                emoji: self.emoji,
            },
            mode_source,
            origin,
        }
    }

    /// Find the first site rule whose key is a substring of the title
    /// (both compared lowercase). Longest key wins on multiple matches
    /// so that e.g. "google docs" beats "google".
    fn site_config_for_title(&self, title: &str) -> Option<&AppConfig> {
        let title_lower = title.to_lowercase();
        self.site_configs
            .iter()
            .filter(|(key, _)| title_lower.contains(&key.to_lowercase()))
            .max_by_key(|(key, _)| key.len())
            .map(|(_, cfg)| cfg)
    }

}
