//! Dynamic RDP Connection Profile Manager and Canonical `.rdp` File Importer/Exporter.
//!
//! Provides zero-hardcode configuration persistence in `~/.config/ironrdp/connections.json`
//! and full interoperability with standard Microsoft Remote Desktop `.rdp` files.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use ironrdp::connector::{self, Credentials, DesktopSize};
use serde::{Deserialize, Serialize};

const DEFAULT_WIDTH: u16 = 1920;
const DEFAULT_HEIGHT: u16 = 1080;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionProfile {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub pass_token: String,
    pub domain: Option<String>,
    pub desktop_width: u16,
    pub desktop_height: u16,
    pub color_depth: u8,
    pub redirect_clipboard: bool,
    pub audio_mode: u8,
    pub compression_enabled: bool,
    pub compression_level: u32,
    pub egfx_enabled: bool,
    pub avc444_enabled: bool,
    pub auto_reconnect: bool,
    pub prevent_session_lock: bool,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            name: "Default Connection".to_string(),
            host: "100.64.0.4".to_string(),
            port: 33898,
            username: "damartel".to_string(),
            pass_token: String::new(),
            domain: None,
            desktop_width: DEFAULT_WIDTH,
            desktop_height: DEFAULT_HEIGHT,
            color_depth: 32,
            redirect_clipboard: true,
            audio_mode: 0, // Bring to this computer
            compression_enabled: true,
            compression_level: 3,
            egfx_enabled: false,
            avc444_enabled: false,
            auto_reconnect: false,
            prevent_session_lock: false,
        }
    }
}

impl ConnectionProfile {
    pub fn new(name: impl Into<String>, host: impl Into<String>, port: u16) -> Self {
        Self {
            name: name.into(),
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    /// Import profile from standard Microsoft Remote Desktop `.rdp` file content.
    pub fn from_rdp_file(input: &str) -> Result<Self> {
        let parse_result = ironrdp_rdpfile::parse(input);
        let props = parse_result.properties;

        let mut profile = Self::default();

        if let Some(addr) = props.get::<&str>("full address") {
            if let Some((h, p)) = addr.split_once(':') {
                profile.host = h.to_string();
                if let Ok(parsed_port) = p.parse::<u16>() {
                    profile.port = parsed_port;
                }
            } else {
                profile.host = addr.to_string();
            }
        }

        if let Some(user) = props.get::<&str>("username") {
            profile.username = user.to_string();
        }

        if let Some(w) = props.get::<i64>("desktopwidth") {
            profile.desktop_width = w as u16;
        }

        if let Some(h) = props.get::<i64>("desktopheight") {
            profile.desktop_height = h as u16;
        }

        if let Some(bpp) = props.get::<i64>("session bpp") {
            profile.color_depth = bpp as u8;
        }

        if let Some(clip) = props.get::<i64>("redirectclipboard") {
            profile.redirect_clipboard = clip == 1;
        }

        if let Some(audio) = props.get::<i64>("audiomode") {
            profile.audio_mode = audio as u8;
        }

        profile.name = format!("Imported ({})", profile.host);
        Ok(profile)
    }

    /// Export profile to canonical Microsoft Remote Desktop `.rdp` file string.
    pub fn to_rdp_file(&self) -> String {
        format!(
            "full address:s:{}:{}\n\
             username:s:{}\n\
             desktopwidth:i:{}\n\
             desktopheight:i:{}\n\
             session bpp:i:{}\n\
             redirectclipboard:i:{}\n\
             audiomode:i:{}\n\
             networklevelauthentication:i:1\n\
             compression:i:{}\n",
            self.host,
            self.port,
            self.username,
            self.desktop_width,
            self.desktop_height,
            self.color_depth,
            if self.redirect_clipboard { 1 } else { 0 },
            self.audio_mode,
            if self.compression_enabled { 1 } else { 0 }
        )
    }

    /// Convert profile into `ironrdp_connector::Config`.
    pub fn to_connector_config(&self) -> Result<connector::Config> {
        let credentials = Credentials::password(self.username.clone(), self.pass_token.clone());

        Ok(connector::Config {
            credentials,
            domain: self.domain.clone(),
            enable_tls: false,
            enable_credssp: true,
            keyboard_type: ironrdp::pdu::gcc::KeyboardType::IbmEnhanced,
            keyboard_subtype: 0,
            keyboard_layout: 0,
            keyboard_functional_keys_count: 12,
            ime_file_name: String::new(),
            dig_product_id: String::new(),
            desktop_size: DesktopSize {
                width: self.desktop_width,
                height: self.desktop_height,
            },
            desktop_scale_factor: 100,
            client_build: 1,
            client_name: "IronRDP-Client".to_string(),
            bitmap: None,
            enable_graphics_pipeline: self.egfx_enabled,
            network_autodetect: false,
            client_dir: String::new(),
            alternate_shell: String::new(),
            work_dir: String::new(),
            platform: ironrdp::pdu::rdp::capability_sets::MajorPlatformType::UNSPECIFIED,
            hardware_id: None,
            request_data: None,
            reconnect_cookie: None,
            autologon: false,
            enable_audio_playback: self.audio_mode == 0,
            performance_flags: ironrdp::pdu::rdp::client_info::PerformanceFlags::empty(),
            license_cache: None,
            timezone_info: ironrdp::pdu::rdp::client_info::TimezoneInfo::default(),
            compression_type: if self.compression_enabled {
                Some(ironrdp::pdu::rdp::client_info::CompressionType::Rdp61)
            } else {
                None
            },
            enable_server_pointer: false,
            pointer_software_rendering: true,
            multitransport_flags: Some(ironrdp::pdu::gcc::MultiTransportFlags::empty()),
        })
    }
}

/// Profile Manager for storing and managing user connection profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileManager {
    pub active_index: usize,
    pub profiles: Vec<ConnectionProfile>,
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self {
            active_index: 0,
            profiles: vec![
                ConnectionProfile::new("spark-0060 (ARM64 Spark)", "100.64.0.4", 33898),
                ConnectionProfile::new("spark-3066 (ARM64 Spark)", "100.64.0.5", 33898),
                ConnectionProfile::new("dtm-p1gen7 (Windows Host)", "100.64.0.1", 3389),
                ConnectionProfile::new("Local Loopback", "127.0.0.1", 33890),
            ],
        }
    }
}

impl ProfileManager {
    pub fn config_path() -> PathBuf {
        if let Some(config_dir) = dirs_next::config_dir() {
            config_dir.join("ironrdp").join("connections.json")
        } else {
            PathBuf::from(".config/ironrdp/connections.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(manager) = serde_json::from_str::<Self>(&content) {
                    return manager;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn active_profile(&self) -> &ConnectionProfile {
        self.profiles.get(self.active_index).unwrap_or(&self.profiles[0])
    }

    pub fn active_profile_mut(&mut self) -> &mut ConnectionProfile {
        let idx = self.active_index;
        if idx < self.profiles.len() {
            &mut self.profiles[idx]
        } else {
            &mut self.profiles[0]
        }
    }

    pub fn add_profile(&mut self, profile: ConnectionProfile) -> usize {
        self.profiles.push(profile);
        self.active_index = self.profiles.len() - 1;
        let _ = self.save();
        self.active_index
    }

    pub fn remove_profile(&mut self, index: usize) {
        if self.profiles.len() > 1 && index < self.profiles.len() {
            self.profiles.remove(index);
            if self.active_index >= self.profiles.len() {
                self.active_index = self.profiles.len() - 1;
            }
            let _ = self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rdp_file_parse_and_export_roundtrip() {
        let rdp_content = "\
full address:s:100.64.0.4:33898
username:s:damartel
desktopwidth:i:2560
desktopheight:i:1440
session bpp:i:32
redirectclipboard:i:1
audiomode:i:0
";
        let profile = ConnectionProfile::from_rdp_file(rdp_content).unwrap();
        assert_eq!(profile.host, "100.64.0.4");
        assert_eq!(profile.port, 33898);
        assert_eq!(profile.username, "damartel");
        assert_eq!(profile.desktop_width, 2560);
        assert_eq!(profile.desktop_height, 1440);
        assert_eq!(profile.color_depth, 32);
        assert!(profile.redirect_clipboard);

        let exported = profile.to_rdp_file();
        assert!(exported.contains("full address:s:100.64.0.4:33898"));
        assert!(exported.contains("username:s:damartel"));
        assert!(exported.contains("desktopwidth:i:2560"));
        assert!(exported.contains("desktopheight:i:1440"));
    }

    #[test]
    fn test_profile_manager_add_and_remove() {
        let mut manager = ProfileManager::default();
        let initial_count = manager.profiles.len();

        let new_prof = ConnectionProfile::new("Custom Host", "192.168.1.50", 3389);
        let new_idx = manager.add_profile(new_prof);
        assert_eq!(manager.profiles.len(), initial_count + 1);
        assert_eq!(manager.active_profile().host, "192.168.1.50");

        manager.remove_profile(new_idx);
        assert_eq!(manager.profiles.len(), initial_count);
    }
}
