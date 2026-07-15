//! Tray helpers extracted from tray.rs (R4).

use std::path::PathBuf;

use crate::config::{ConfigManager, ImeMode, InputMethod};
use tracing::{info, warn};
use super::{ICON_ON, ICON_OFF, ICON_ON_SVG, ICON_OFF_SVG};

pub(crate) fn install_icons() -> Option<PathBuf> {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok()?;
    let dir = base.join("vi-im/icons");
    std::fs::create_dir_all(&dir).ok()?;
    for (name, svg) in [(ICON_ON, ICON_ON_SVG), (ICON_OFF, ICON_OFF_SVG)] {
        let path = dir.join(format!("{name}.svg"));
        if std::fs::write(&path, svg).is_err() {
            warn!("[TRAY] could not install icon {:?}", path);
        } else {
            info!("[TRAY] installed icon {:?} ({} bytes)", path, svg.len());
        }
    }
    Some(dir)
}

/// Read tray-relevant config from disk.
pub(crate) fn read_state(path: &PathBuf) -> (InputMethod, ImeMode, bool) {
    ConfigManager::new(Some(path.clone()))
        .map(|m| {
            let s = m.setting();
            (s.input_method, s.ime_mode, s.enabled)
        })
        .unwrap_or((InputMethod::Telex, ImeMode::Preedit, true))
}

/// Convert InputMethod to display label.
pub(crate) fn method_label(method: InputMethod) -> &'static str {
    match method {
        InputMethod::Telex => "Telex",
        InputMethod::Vni => "VNI",
        InputMethod::Smart => "Tự do",
    }
}

/// Convert ImeMode to display label.
pub(crate) fn mode_label(mode: ImeMode) -> &'static str {
    match mode {
        ImeMode::Preedit => "Preedit (gạch chân)",
        ImeMode::NonPreedit => "NonPreedit (gõ thẳng)",
    }
}
