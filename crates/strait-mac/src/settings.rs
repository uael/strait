//! Choices the user makes, kept across launches.

use std::path::Path;

use serde::{Deserialize, Serialize};
use strait::wire::Preset;

use crate::core::{preset_name, preset_named};

/// Monitor input codes (DDC feature 0x60) of the Samsung Odyssey G9: HDMI,
/// where the Mac is, and DisplayPort, where Windows is.
pub const MAC_INPUT: u16 = 0x05;
pub const WINDOWS_INPUT: u16 = 0x0F;

#[derive(Debug)]
pub struct Settings {
    pub preset: Preset,
    pub swap_command_control: bool,
    /// The PC's name or address; found by its beacon when absent.
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    preset: Option<String>,
    swap_command_control: bool,
    host: Option<String>,
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let stored: Stored = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Settings {
            preset: stored.preset.as_deref().and_then(preset_named).unwrap_or(Preset::MacFull),
            swap_command_control: stored.swap_command_control,
            host: stored.host,
        }
    }

    pub fn save(&self, path: &Path) {
        let stored = Stored {
            preset: Some(preset_name(self.preset).to_owned()),
            swap_command_control: self.swap_command_control,
            host: self.host.clone(),
        };
        let json = serde_json::to_vec_pretty(&stored).expect("settings serialize");
        if let Err(e) = std::fs::write(path, json) {
            crate::log(&format!("save settings to {}: {e}", path.display()));
        }
    }
}
