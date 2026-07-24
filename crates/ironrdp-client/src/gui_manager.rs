//! Authentic Microsoft Remote Desktop Connection (`mstsc.exe`) GUI Launcher.
//!
//! Provides a tabbed connection dialog with profile management, canonical `.rdp` file
//! import/export, display configuration, local resource redirection, and performance presets.

use std::fs;
use std::path::PathBuf;

use crate::profile::{ConnectionProfile, ProfileManager};
use egui::{Color32, Context, RichText, Ui, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MstscTab {
    General,
    Display,
    LocalResources,
    Experience,
    Advanced,
}

#[derive(Debug, Clone)]
pub enum LauncherOutcome {
    None,
    Connect(ConnectionProfile),
    Cancel,
}

pub struct MstscGui {
    pub profile_manager: ProfileManager,
    pub active_tab: MstscTab,
    pub show_options: bool,
    pub custom_host_input: String,
    pub custom_username_input: String,
    pub custom_password_input: String,
    pub status_message: String,
    pub is_probing_host: bool,
    pub host_online: bool,
    pub outcome: LauncherOutcome,
}

impl Default for MstscGui {
    fn default() -> Self {
        let manager = ProfileManager::load();
        let active_prof = manager.active_profile().clone();

        Self {
            profile_manager: manager,
            active_tab: MstscTab::General,
            show_options: true,
            custom_host_input: format!("{}:{}", active_prof.host, active_prof.port),
            custom_username_input: active_prof.username.clone(),
            custom_password_input: active_prof.pass_token.clone(),
            status_message: "Ready to connect".to_string(),
            is_probing_host: false,
            host_online: true,
            outcome: LauncherOutcome::None,
        }
    }
}

impl MstscGui {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sync_inputs_from_active_profile(&mut self) {
        let active_prof = self.profile_manager.active_profile().clone();
        self.custom_host_input = format!("{}:{}", active_prof.host, active_prof.port);
        self.custom_username_input = active_prof.username.clone();
        self.custom_password_input = active_prof.pass_token.clone();
    }

    pub fn sync_active_profile_from_inputs(&mut self) {
        let host_input = self.custom_host_input.clone();
        let username_input = self.custom_username_input.clone();
        let password_input = self.custom_password_input.clone();

        let prof = self.profile_manager.active_profile_mut();
        if let Some((h, p)) = host_input.split_once(':') {
            prof.host = h.to_string();
            if let Ok(parsed_port) = p.parse::<u16>() {
                prof.port = parsed_port;
            }
        } else {
            prof.host = host_input;
        }
        prof.username = username_input;
        prof.pass_token = password_input;
    }

    pub fn show(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_mstsc_window(ui);
        });
    }

    fn render_mstsc_window(&mut self, ui: &mut Ui) {
        ui.style_mut().spacing.item_spacing = Vec2::new(8.0, 6.0);

        // Header Banner
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("💻 Remote Desktop Connection")
                    .strong()
                    .color(Color32::from_rgb(30, 90, 180)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("IronRDP Client v0.14.0").small().color(Color32::GRAY));
            });
        });
        ui.separator();

        if self.show_options {
            // Tab Bar
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, MstscTab::General, "General");
                ui.selectable_value(&mut self.active_tab, MstscTab::Display, "Display");
                ui.selectable_value(&mut self.active_tab, MstscTab::LocalResources, "Local Resources");
                ui.selectable_value(&mut self.active_tab, MstscTab::Experience, "Experience");
                ui.selectable_value(&mut self.active_tab, MstscTab::Advanced, "Advanced");
            });
            ui.separator();

            match self.active_tab {
                MstscTab::General => self.render_general_tab(ui),
                MstscTab::Display => self.render_display_tab(ui),
                MstscTab::LocalResources => self.render_local_resources_tab(ui),
                MstscTab::Experience => self.render_experience_tab(ui),
                MstscTab::Advanced => self.render_advanced_tab(ui),
            }
        } else {
            // Collapsed Compact View
            self.render_compact_view(ui);
        }

        ui.separator();

        // Footer Action Bar
        ui.horizontal(|ui| {
            let options_btn_text = if self.show_options {
                "Hide Options ^"
            } else {
                "Show Options v"
            };
            if ui.button(options_btn_text).clicked() {
                self.show_options = !self.show_options;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new("Connect").strong().color(Color32::WHITE))
                    .clicked()
                {
                    self.sync_active_profile_from_inputs();
                    let prof = self.profile_manager.active_profile().clone();
                    self.outcome = LauncherOutcome::Connect(prof);
                }

                if ui.button("Cancel").clicked() {
                    self.outcome = LauncherOutcome::Cancel;
                }

                if ui.button("Help").clicked() {
                    self.status_message = "Help: Enter host:port and click Connect".to_string();
                }
            });
        });

        // Status Line
        ui.horizontal(|ui| {
            let status_color = if self.host_online {
                Color32::from_rgb(0, 160, 80)
            } else {
                Color32::from_rgb(200, 50, 50)
            };
            ui.label(
                RichText::new(if self.host_online { "● Online" } else { "○ Offline" })
                    .color(status_color)
                    .small(),
            );
            ui.label(RichText::new(&self.status_message).small().italics());
        });
    }

    fn render_compact_view(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label(RichText::new("Logon settings").strong());
            ui.label("Enter the name of the remote computer.");

            egui::Grid::new("compact_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Computer:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.custom_host_input);
                        if ui.button("Add New...").clicked() {
                            let new_prof = ConnectionProfile::new("Custom Host", "127.0.0.1", 33890);
                            let idx = self.profile_manager.add_profile(new_prof);
                            self.profile_manager.active_index = idx;
                            self.sync_inputs_from_active_profile();
                        }
                    });
                    ui.end_row();

                    ui.label("User name:");
                    ui.text_edit_singleline(&mut self.custom_username_input);
                    ui.end_row();
                });
        });
    }

    fn render_general_tab(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label(RichText::new("Logon settings").strong());
            ui.label("Specify connection target and credentials.");

            egui::Grid::new("general_logon_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Saved Profiles:");
                    egui::ComboBox::from_label("")
                        .selected_text(self.profile_manager.active_profile().name.clone())
                        .show_ui(ui, |ui| {
                            for i in 0..self.profile_manager.profiles.len() {
                                let name = self.profile_manager.profiles[i].name.clone();
                                if ui
                                    .selectable_value(&mut self.profile_manager.active_index, i, name)
                                    .clicked()
                                {
                                    self.sync_inputs_from_active_profile();
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Computer:");
                    ui.text_edit_singleline(&mut self.custom_host_input);
                    ui.end_row();

                    ui.label("User name:");
                    ui.text_edit_singleline(&mut self.custom_username_input);
                    ui.end_row();

                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.custom_password_input).password(true));
                    ui.end_row();
                });
        });

        ui.add_space(6.0);

        ui.group(|ui| {
            ui.label(RichText::new("Connection settings").strong());
            ui.label("Save current settings to an RDP file or open an existing configuration.");

            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.sync_active_profile_from_inputs();
                    let _ = self.profile_manager.save();
                    self.status_message = "Profile saved to ~/.config/ironrdp/connections.json".to_string();
                }

                if ui.button("Save As...").clicked() {
                    self.sync_active_profile_from_inputs();
                    let prof = self.profile_manager.active_profile();
                    let rdp_content = prof.to_rdp_file();
                    let target_path = PathBuf::from("/tmp/connection.rdp");
                    let _ = fs::write(&target_path, rdp_content);
                    self.status_message = format!("Exported .rdp to {}", target_path.display());
                }

                if ui.button("Open .rdp File...").clicked() {
                    let target_path = PathBuf::from("/tmp/connection.rdp");
                    if target_path.exists() {
                        if let Ok(content) = fs::read_to_string(&target_path) {
                            if let Ok(imported) = ConnectionProfile::from_rdp_file(&content) {
                                let idx = self.profile_manager.add_profile(imported);
                                self.profile_manager.active_index = idx;
                                self.sync_inputs_from_active_profile();
                                self.status_message = "Loaded .rdp file successfully".to_string();
                            }
                        }
                    } else {
                        self.status_message = "File /tmp/connection.rdp not found".to_string();
                    }
                }
            });
        });
    }

    fn render_display_tab(&mut self, ui: &mut Ui) {
        let prof = self.profile_manager.active_profile_mut();

        ui.group(|ui| {
            ui.label(RichText::new("Display configuration").strong());
            ui.label("Set the display size of your remote desktop.");

            ui.horizontal(|ui| {
                ui.label("Resolution:");
                ui.add(egui::Slider::new(&mut prof.desktop_width, 640..=3840).text("Width"));
                ui.add(egui::Slider::new(&mut prof.desktop_height, 480..=2160).text("Height"));
            });

            ui.horizontal(|ui| {
                if ui.button("1080p (1920x1080)").clicked() {
                    prof.desktop_width = 1920;
                    prof.desktop_height = 1080;
                }
                if ui.button("1440p (2560x1440)").clicked() {
                    prof.desktop_width = 2560;
                    prof.desktop_height = 1440;
                }
                if ui.button("4K (3840x2160)").clicked() {
                    prof.desktop_width = 3840;
                    prof.desktop_height = 2160;
                }
            });
        });

        ui.add_space(6.0);

        ui.group(|ui| {
            ui.label(RichText::new("Colors").strong());
            ui.label("Select the color depth for the remote session.");

            ui.horizontal(|ui| {
                ui.label("Color Depth:");
                egui::ComboBox::from_label("")
                    .selected_text(format!("{} bit", prof.color_depth))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut prof.color_depth, 16, "High Color (16 bit)");
                        ui.selectable_value(&mut prof.color_depth, 24, "True Color (24 bit)");
                        ui.selectable_value(&mut prof.color_depth, 32, "Highest Quality (32 bit)");
                    });
            });
        });
    }

    fn render_local_resources_tab(&mut self, ui: &mut Ui) {
        let prof = self.profile_manager.active_profile_mut();

        ui.group(|ui| {
            ui.label(RichText::new("Remote audio").strong());
            ui.label("Configure remote audio playback destination.");

            ui.radio_value(&mut prof.audio_mode, 0, "Play on this computer (Bring to this device)");
            ui.radio_value(&mut prof.audio_mode, 1, "Do not play");
            ui.radio_value(&mut prof.audio_mode, 2, "Play on remote computer");
        });

        ui.add_space(6.0);

        ui.group(|ui| {
            ui.label(RichText::new("Local devices and resources").strong());
            ui.checkbox(
                &mut prof.redirect_clipboard,
                "Clipboard sharing (copy & paste text/data)",
            );
        });
    }

    fn render_experience_tab(&mut self, ui: &mut Ui) {
        let prof = self.profile_manager.active_profile_mut();

        ui.group(|ui| {
            ui.label(RichText::new("Performance & Acceleration").strong());
            ui.label("Choose your connection speed to optimize performance.");

            ui.horizontal(|ui| {
                if ui.button("10G LAN Preset").clicked() {
                    prof.egfx_enabled = true;
                    prof.avc444_enabled = true;
                    prof.compression_enabled = true;
                    prof.compression_level = 3;
                }
                if ui.button("Tailscale WAN Preset").clicked() {
                    prof.egfx_enabled = false;
                    prof.compression_enabled = true;
                    prof.compression_level = 3;
                }
            });

            ui.separator();
            ui.checkbox(&mut prof.compression_enabled, "Enable RDP Bulk Compression");
            ui.checkbox(&mut prof.egfx_enabled, "Enable RDPEGFX Graphics Pipeline");
            ui.checkbox(&mut prof.avc444_enabled, "Enable AVC444 High-Color H.264 Acceleration");
            ui.checkbox(&mut prof.auto_reconnect, "Auto-reconnect on network drop");
            ui.checkbox(&mut prof.prevent_session_lock, "Prevent remote session idle lock");
        });
    }

    fn render_advanced_tab(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.label(RichText::new("Server authentication & Fleet Tooling").strong());
            ui.label("Verify server identity and manage remote server state.");

            if ui.button("⚡ Trigger Remote Spark Auto-Start (SSH / WoL)").clicked() {
                self.status_message = "Initiating remote server start over SSH...".to_string();
                let host_target = self.custom_host_input.clone();
                std::thread::spawn(move || {
                    let mut parts = host_target.split(':');
                    let host = parts.next().unwrap_or("100.64.0.4");
                    let port = parts.next().unwrap_or("33898");
                    let _ = std::process::Command::new(
                        "/home/damartel/dev/repos/IronRDP/scripts/fleet/start_spark_server.sh",
                    )
                    .arg(host)
                    .arg(port)
                    .status();
                });
            }
        });
    }

    pub fn render_to_framebuffer(&mut self, ctx: &Context, width: usize, height: usize, buffer: &mut [u32]) {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(width as f32, height as f32),
            )),
            ..Default::default()
        };

        let full_output = ctx.run(raw_input, |ctx| {
            self.show(ctx);
        });

        let primitives = ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        // Clear background (#1A1D24)
        for pixel in buffer.iter_mut() {
            *pixel = 0xFF1A1D24;
        }

        for clipped_prim in primitives {
            if let egui::epaint::Primitive::Mesh(mesh) = clipped_prim.primitive {
                let rect = clipped_prim.clip_rect;
                let min_x = (rect.min.x as usize).min(width);
                let max_x = (rect.max.x as usize).min(width);
                let min_y = (rect.min.y as usize).min(height);
                let max_y = (rect.max.y as usize).min(height);

                for i in (0..mesh.indices.len()).step_by(3) {
                    if i + 2 >= mesh.indices.len() {
                        break;
                    }
                    let idx0 = mesh.indices[i] as usize;
                    let idx1 = mesh.indices[i + 1] as usize;
                    let idx2 = mesh.indices[i + 2] as usize;

                    if idx0 >= mesh.vertices.len() || idx1 >= mesh.vertices.len() || idx2 >= mesh.vertices.len() {
                        continue;
                    }

                    let v0 = &mesh.vertices[idx0];
                    let v1 = &mesh.vertices[idx1];
                    let v2 = &mesh.vertices[idx2];

                    let p0 = v0.pos;
                    let p1 = v1.pos;
                    let p2 = v2.pos;

                    let tri_min_x = (p0.x.min(p1.x).min(p2.x) as usize).max(min_x).min(max_x);
                    let tri_max_x = (p0.x.max(p1.x).max(p2.x) as usize).max(min_x).min(max_x);
                    let tri_min_y = (p0.y.min(p1.y).min(p2.y) as usize).max(min_y).min(max_y);
                    let tri_max_y = (p0.y.max(p1.y).max(p2.y) as usize).max(min_y).min(max_y);

                    let color = v0.color;
                    let argb = ((color.a() as u32) << 24)
                        | ((color.r() as u32) << 16)
                        | ((color.g() as u32) << 8)
                        | (color.b() as u32);

                    for py in tri_min_y..tri_max_y {
                        for px in tri_min_x..tri_max_x {
                            let idx = py * width + px;
                            if idx < buffer.len() {
                                buffer[idx] = argb;
                            }
                        }
                    }
                }
            }
        }
    }
}
