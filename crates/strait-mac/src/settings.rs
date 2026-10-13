//! Choices the user makes, kept across launches.

use std::path::Path;

use serde::{Deserialize, Serialize};
use strait::wire::Preset;

use crate::core::{preset_name, preset_named};

/// Monitor input codes (DDC feature 0x60) of the Samsung Odyssey G9: HDMI,
/// where the Mac is, and DisplayPort, where Windows is.
pub const MAC_INPUT: u16 = 0x05;
pub const WINDOWS_INPUT: u16 = 0x0F;

/// Bounds of the pointer speed on Windows, a factor on the Mac's motion.
pub const MIN_SPEED: f64 = 0.1;
pub const MAX_SPEED: f64 = 2.0;
/// Raw counts of a modern mouse move the Windows pointer much faster than
/// macOS moves its own; half speed is closer to the Mac's feel.
const DEFAULT_SPEED: f64 = 0.5;

#[derive(Debug)]
pub struct Settings {
    pub preset: Preset,
    pub swap_command_control: bool,
    /// Factor on pointer motion sent to Windows.
    pub pointer_speed: f64,
    /// The PC's name or address; found by its beacon when absent.
    pub host: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    preset: Option<String>,
    swap_command_control: bool,
    pointer_speed: Option<f64>,
    host: Option<String>,
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let stored: Stored = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Settings {
            // Only a full-screen choice is remembered; side by side follows
            // the monitor's PBP mode.
            preset: match stored.preset.as_deref().and_then(preset_named) {
                Some(Preset::WindowsFull) => Preset::WindowsFull,
                _ => Preset::MacFull,
            },
            swap_command_control: stored.swap_command_control,
            pointer_speed: stored.pointer_speed.unwrap_or(DEFAULT_SPEED).clamp(MIN_SPEED, MAX_SPEED),
            host: stored.host,
        }
    }

    pub fn save(&self, path: &Path) {
        let stored = Stored {
            preset: Some(preset_name(self.preset).to_owned()),
            swap_command_control: self.swap_command_control,
            pointer_speed: Some(self.pointer_speed),
            host: self.host.clone(),
        };
        let json = serde_json::to_vec_pretty(&stored).expect("settings serialize");
        if let Err(e) = std::fs::write(path, json) {
            crate::log(&format!("save settings to {}: {e}", path.display()));
        }
    }
}
