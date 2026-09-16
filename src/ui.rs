//! Settings window.
//!
//! Built on `eframe`/`egui`, styled to sit comfortably next to Windows 11's own
//! settings pages: Segoe UI for Latin text with Microsoft YaHei as the CJK
//! fallback, rounded corners applied through DWM, and the same light/dark mode
//! the rest of the system is using.
//!
//! The UI thread is the only writer of the configuration file; the tray menu
//! communicates through flags.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

use crate::catalog;
use crate::config::{self, Action, BatteryAction, Config, ExternalRate, HapticsConfig};
use crate::state::Shared;

const WINDOW_TITLE: &str = "MP14Tools";
/// `DWMWA_USE_IMMERSIVE_DARK_MODE`.
const DWMWA_DARK_MODE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(20);
/// `DWMWA_WINDOW_CORNER_PREFERENCE`.
const DWMWA_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
/// `DWMWCP_ROUND`.
const DWMWCP_ROUND: i32 = 2;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(450);

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Touchpad,
    OemKeys,
    Display,
    Log,
    General,
}

pub struct SettingsApp {
    shared: Arc<Shared>,
    /// Editable copy; pushed to `shared` and to disk after a short debounce.
    working: Config,
    tab: Tab,
    dirty_since: Option<Instant>,
    seen_generation: u64,
    status: String,
    autostart_enabled: bool,
    dark: bool,
    window_decorated: bool,
    /// Pressure thresholds as currently shown in the boxes. They only reach
    /// `working` once the matching "确定" button is pressed.
    light_pending: i32,
    deep_pending: i32,
}

impl SettingsApp {
    pub fn new(context: &eframe::CreationContext<'_>, shared: Arc<Shared>) -> Self {
        install_fonts(&context.egui_ctx);
        let dark = !system_uses_light_theme();
        apply_visuals(&context.egui_ctx, dark);

        // Let the tray thread nudge this event loop instead of the UI polling.
        let wake_context = context.egui_ctx.clone();
        shared.set_wake_ui(Box::new(move || wake_context.request_repaint()));

        let working = shared
            .config
            .read()
            .map(|config| config.clone())
            .unwrap_or_default();

        let light_pending = working.touchpad.light_press_threshold as i32;
        let deep_pending = working.touchpad.deep_press_threshold as i32;

        Self {
            shared,
            working,
            tab: Tab::Touchpad,
            dirty_since: None,
            seen_generation: 0,
            status: String::new(),
            autostart_enabled: crate::autostart::is_enabled(),
            dark,
            window_decorated: false,
            light_pending,
            deep_pending,
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty_since = Some(Instant::now());
    }

    /// Flush the working copy to `shared` and to disk.
    fn save(&mut self) {
        self.dirty_since = None;

        let text = match serde_json::to_string_pretty(&self.working) {
            Ok(text) => text,
            Err(error) => {
                self.status = format!("配置序列化失败：{error}");
                return;
            }
        };

        if let Err(error) = std::fs::create_dir_all(config::data_dir()) {
            self.status = format!("无法创建配置目录：{error}");
            return;
        }
        if let Err(error) = std::fs::write(config::config_path(), &text) {
            self.status = format!("配置写入失败：{error}");
            return;
        }

        // Tell the watcher this text is ours so it does not reload it back.
        if let Ok(mut applied) = self.shared.config_text.lock() {
            *applied = text;
        }
        if let Ok(mut slot) = self.shared.config.write() {
            *slot = self.working.clone();
        }

        crate::tray::apply_visibility(&self.shared);
        // The options that act outside this process take effect right away
        // instead of waiting for the next poll of the file.
        crate::apply_log_settings(&self.shared);
        crate::haptics::request_from(&self.shared);
        self.status = "已保存".to_string();
    }

    fn sync_from_disk(&mut self) {
        if let Ok(slot) = self.shared.config.read() {
            self.working = slot.clone();
        }
        self.light_pending = self.working.touchpad.light_press_threshold as i32;
        self.deep_pending = self.working.touchpad.deep_press_threshold as i32;
        self.autostart_enabled = crate::autostart::is_enabled();
        self.status = "已重新加载配置文件".to_string();
    }

    fn handle_shared_signals(&mut self, context: &egui::Context) {
        if self
            .shared
            .autostart_toggle_requested
            .swap(false, Ordering::SeqCst)
        {
            let target = !self.autostart_enabled;
            self.working.start_with_windows = target;
            self.autostart_enabled = target;
            self.apply_autostart(target);
            self.mark_dirty();
        }

        if self.shared.console_toggle_requested.swap(false, Ordering::SeqCst) {
            let target = !self.working.log.console;
            self.working.log.console = target;
            crate::console::sync(target);
            self.status = if target {
                "已打开命令提示符".to_string()
            } else {
                "已关闭命令提示符".to_string()
            };
            self.mark_dirty();
        }

        if self.shared.show_requested.swap(false, Ordering::SeqCst) {
            context.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if self.shared.exit_requested.load(Ordering::SeqCst) {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let generation = self.shared.config_generation.load(Ordering::SeqCst);
        if generation != self.seen_generation {
            self.seen_generation = generation;
            // Do not fight in-progress edits.
            if self.dirty_since.is_none() {
                self.sync_from_disk();
            }
        }
    }

    fn apply_autostart(&mut self, enabled: bool) {
        match crate::autostart::set_enabled(enabled) {
            Ok(()) => {
                self.status = if enabled {
                    "已设置开机自启".to_string()
                } else {
                    "已取消开机自启".to_string()
                };
            }
            Err(error) => self.status = format!("开机自启设置失败：{error}"),
        }
    }

    fn decorate_window(&mut self, context: &egui::Context) {
        if self.window_decorated {
            return;
        }

        context.request_repaint();
        let title = crate::win::wide(WINDOW_TITLE);
        unsafe {
            let Ok(window) = FindWindowW(None, PCWSTR(title.as_ptr())) else {
                return;
            };
            self.window_decorated = true;
            apply_dwm(window, DWMWA_CORNER_PREFERENCE, &DWMWCP_ROUND);
            let dark = BOOL::from(self.dark);
            apply_dwm(window, DWMWA_DARK_MODE, &dark);
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.label(egui::RichText::new("MP14Tools").size(18.0).strong());
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                .size(11.5)
                .weak(),
        );
        ui.add_space(18.0);

        for (tab, label) in [
            (Tab::Touchpad, "触摸板"),
            (Tab::OemKeys, "OEM 按键"),
            (Tab::Display, "显示"),
            (Tab::Log, "日志"),
            (Tab::General, "通用"),
        ] {
            if ui.selectable_label(self.tab == tab, label).clicked() {
                self.tab = tab;
                self.status.clear();
            }
            ui.add_space(4.0);
        }

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(12.0);
            let paused = self.shared.is_paused();
            ui.label(
                egui::RichText::new(if paused { "已暂停映射" } else { "运行中" })
                    .size(12.0)
                    .color(if paused {
                        egui::Color32::from_rgb(0xD1, 0x74, 0x2B)
                    } else {
                        egui::Color32::from_rgb(0x2E, 0x9E, 0x5B)
                    }),
            );
            ui.label(egui::RichText::new(&self.status).size(11.5).weak());
        });
    }

    fn touchpad_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("触摸板");
        ui.label(
            egui::RichText::new("把触摸板的重按映射成任意按键、鼠标键或组合键。")
                .size(12.5)
                .weak(),
        );
        ui.add_space(12.0);

        let mut changed = false;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            changed |= ui
                .checkbox(&mut self.working.touchpad.enabled, "启用重按检测")
                .changed();

            ui.add_space(8.0);
            pressure_readout(ui, &self.shared);
        });

        ui.add_space(10.0);

        // Edited on a copy and written back only when the matching "确定" is
        // pressed, so a half-typed number can never change the behaviour.
        let mut factory = self.working.touchpad.factory_values;
        let light = self.working.touchpad.light_press_threshold as i32;
        let deep = self.working.touchpad.deep_press_threshold as i32;
        let mut light_pending = self.light_pending;
        let mut deep_pending = self.deep_pending;
        let mut light_confirmed = false;
        let mut deep_confirmed = false;

        let min_light = config::MIN_LIGHT_PRESS_THRESHOLD as i32;
        let max_light = config::MAX_LIGHT_PRESS_THRESHOLD as i32;
        let max_deep = config::MAX_DEEP_PRESS_THRESHOLD as i32;

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("按压力度阈值（HID 原始压力值，整数）").strong());
            ui.label(
                egui::RichText::new("拖动滑条或在数值框里输入，按对应「确定」后生效。")
                    .size(11.5)
                    .weak(),
            );
            ui.add_space(10.0);

            ui.checkbox(&mut factory, "使用出厂值")
                .on_hover_text("勾选后两个阈值固定为出厂值，下面的编辑区不再生效");
            ui.label(
                egui::RichText::new(format!(
                    "出厂值就是这块触摸板原厂的判定点：轻按 {}、重按 {}。\
                     勾选表示一直使用它；取消勾选后可自行微调，两档之间至少相差 1。",
                    config::FACTORY_LIGHT_PRESS_THRESHOLD,
                    config::FACTORY_DEEP_PRESS_THRESHOLD,
                ))
                .size(11.5)
                .weak(),
            );

            if factory {
                return;
            }

            ui.add_space(14.0);

            // The light threshold only decides what counts as a press, so it has
            // to stay below the one that fires.
            let light_max = (deep - 1).clamp(min_light, max_light);
            light_confirmed = threshold_editor(
                ui,
                "light",
                "轻按",
                &format!("判定为有意按压的压力（{min_light}–{light_max}，整数）"),
                &mut light_pending,
                light,
                Scale::thresholds(min_light, light_max),
            );

            ui.add_space(16.0);

            deep_confirmed = threshold_editor(
                ui,
                "deep",
                "重按",
                &format!(
                    "达到该压力并保持两帧即触发动作（{}–{max_deep}，整数）",
                    light + 1
                ),
                &mut deep_pending,
                deep,
                Scale::thresholds(light + 1, max_deep),
            );
        });

        self.light_pending = light_pending;
        self.deep_pending = deep_pending;

        if factory != self.working.touchpad.factory_values {
            self.working.touchpad.factory_values = factory;
            // Normalising is what applies the factory values, and what brings the
            // editable copies back in line when the switch is turned off again.
            self.working.touchpad.normalize();
            self.light_pending = self.working.touchpad.light_press_threshold as i32;
            self.deep_pending = self.working.touchpad.deep_press_threshold as i32;
            changed = true;
        }

        if light_confirmed {
            self.working.touchpad.light_press_threshold = light_pending as u16;
        }
        if deep_confirmed {
            self.working.touchpad.deep_press_threshold = deep_pending as u16;
        }
        if light_confirmed || deep_confirmed {
            self.working.touchpad.normalize();
            self.light_pending = self.working.touchpad.light_press_threshold as i32;
            self.deep_pending = self.working.touchpad.deep_press_threshold as i32;
            changed = true;
        }

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if haptics_editor(ui, &mut self.working.haptics, &self.shared) {
                changed = true;
            }
        });

        ui.add_space(10.0);
        let mut action = self.working.touchpad.action.clone();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("重按触发").strong());
            ui.add_space(6.0);
            if action_editor(ui, "touchpad", &mut action) {
                changed = true;
            }
        });
        self.working.touchpad.action = action;

        if changed {
            self.mark_dirty();
        }
    }

    fn oem_keys_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("OEM 按键");
        ui.label(
            egui::RichText::new("厂商热键通过 WMI HID 事件上报，按报告前缀匹配。")
                .size(12.5)
                .weak(),
        );
        ui.add_space(12.0);

        let mut changed = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for index in 0..self.working.oem_keys.len() {
                let key = &mut self.working.oem_keys[index];
                let title = if key.name.trim().is_empty() {
                    format!("按键 {}", index + 1)
                } else {
                    key.name.clone()
                };
                let summary = crate::input::describe(&key.action);
                let header = if key.enabled {
                    format!("{title}    {summary}")
                } else {
                    format!("{title}    （已停用）")
                };

                egui::CollapsingHeader::new(header)
                    .id_salt(format!("key-{index}"))
                    .show(ui, |ui| {
                        changed |= ui.checkbox(&mut key.enabled, "启用").changed();
                        changed |= ui.checkbox(&mut key.press_only, "仅在按下时触发").changed();

                        ui.horizontal(|ui| {
                            ui.label("报告前缀");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut key.report_hex)
                                        .desired_width(260.0)
                                        .hint_text("01-28-01"),
                                )
                                .changed();
                        });

                        ui.add_space(6.0);
                        let mut action = key.action.clone();
                        if action_editor(ui, &format!("oem-{index}"), &mut action) {
                            key.action = action;
                            changed = true;
                        }
                    });
            }
        });

        if changed {
            self.mark_dirty();
        }
    }

    /// Battery refresh-rate switching and HDR - the behaviour of the reference
    /// machine's "smart refresh rate" tool, plus the switches that decide which
    /// screens it is allowed to touch.
    fn display_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("显示");
        ui.label(
            egui::RichText::new("电池供电时把内屏与外屏切到各自设定的档位，并检查内屏 HDR。")
                .size(12.5)
                .weak(),
        );
        ui.add_space(12.0);

        let mut changed = false;
        let mut display = self.working.display.clone();

        egui::Frame::group(ui.style()).show(ui, |ui| {
            changed |= ui.checkbox(&mut display.enabled, "启用显示调节").changed();
            ui.label(
                egui::RichText::new("关闭时本工具不读取也不修改任何显示设置，其余功能不受影响。")
                    .size(11.5)
                    .weak(),
            );

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label("切到电池供电时：");
                for (action, label, hint) in [
                    (BatteryAction::Off, "不处理", "只在日志里记录，不动显示设置"),
                    (
                        BatteryAction::Notify,
                        "通知确认",
                        "弹出提示，点了按钮才切换",
                    ),
                    (BatteryAction::Force, "直接切换", "立即切换，然后告知结果"),
                ] {
                    let selected = display.battery_action == action;
                    if ui
                        .selectable_label(selected, label)
                        .on_hover_text(hint)
                        .clicked()
                        && !selected
                    {
                        display.battery_action = action;
                        changed = true;
                    }
                }
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label("内屏档位：");
                for rate in crate::config::INTERNAL_RATES {
                    let selected = display.internal_refresh_rate == rate;
                    if ui
                        .selectable_label(selected, format!("{rate} Hz"))
                        .on_hover_text("电池供电时把内屏切到这个档位")
                        .clicked()
                        && !selected
                    {
                        display.internal_refresh_rate = rate;
                        changed = true;
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("外屏档位：");
                for (target, label, hint) in [
                    (
                        ExternalRate::Highest,
                        "最高档",
                        "电池供电时让外屏跑在它的最高可用档",
                    ),
                    (ExternalRate::Hz60, "60 Hz", "电池供电时把外屏切到 60 Hz"),
                ] {
                    let selected = display.external_refresh_rate == target;
                    if ui
                        .selectable_label(selected, label)
                        .on_hover_text(hint)
                        .clicked()
                        && !selected
                    {
                        display.external_refresh_rate = target;
                        changed = true;
                    }
                }
            });

            ui.label(
                egui::RichText::new(
                    "切换时取不高于所选档位的最高可用档；面板若没有这一档则退到最低档。\
                     插回电源时恢复切换前的档位。",
                )
                .size(11.5)
                .weak(),
            );
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("HDR").strong());
            changed |= ui
                .checkbox(
                    &mut display.hdr_check,
                    "电池模式下检测内屏 HDR，并提供「关闭 HDR」按钮",
                )
                .changed();
            ui.label(
                egui::RichText::new(
                    "只针对内屏。切换刷新率的提示会直接带上这个按钮；另外每次从睡眠唤醒（电池供电时）也会检查一次并弹出提示。",
                )
                .size(11.5)
                .weak(),
            );
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("生效范围").strong());
            changed |= ui
                .checkbox(&mut display.internal, "内屏（笔记本自带屏幕）")
                .changed();
            changed |= ui
                .checkbox(&mut display.external, "外屏（外接显示器）")
                .changed();
            ui.label(
                egui::RichText::new(
                    "没有勾选的那一类屏幕不会被切换刷新率；HDR 只针对内屏。",
                )
                .size(11.5)
                .weak(),
            );
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("当前显示器").strong());
            ui.add_space(6.0);

            let displays = crate::display::list_cached();
            if displays.is_empty() {
                ui.label(
                    egui::RichText::new("没有读到活动显示器。")
                        .size(11.5)
                        .weak(),
                );
            }
            for state in &displays {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(if state.internal { "内屏" } else { "外屏" })
                            .size(11.5)
                            .color(if state.internal {
                                egui::Color32::from_rgb(0x1F, 0x6F, 0xEB)
                            } else {
                                egui::Color32::from_rgb(0x2E, 0x9E, 0x5B)
                            }),
                    );
                    ui.label(egui::RichText::new(state.describe()).size(12.0));
                    if state.hdr_supported {
                        ui.label(
                            egui::RichText::new(if state.hdr_enabled {
                                "HDR 已开启"
                            } else {
                                "HDR 已关闭"
                            })
                            .size(11.5)
                            .color(if state.hdr_enabled {
                                egui::Color32::from_rgb(0xD1, 0x74, 0x2B)
                            } else {
                                egui::Color32::from_gray(150)
                            }),
                        );
                    } else {
                        ui.label(egui::RichText::new("不支持 HDR").size(11.5).weak());
                    }
                });
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("刷新列表").clicked() {
                    crate::display::invalidate();
                }
                if ui
                    .button("按当前电源状态执行一次")
                    .on_hover_text("不需要真的插拔电源就能验证设置")
                    .clicked()
                {
                    crate::power::request(crate::power::Event::Manual);
                    self.status = "已请求执行一次显示调节".to_string();
                }
            });

            let last = self.shared.display_status();
            if !last.is_empty() {
                ui.label(
                    egui::RichText::new(format!("最近一次：{last}"))
                        .size(11.5)
                        .weak(),
                );
            }
        });

        if changed {
            self.working.display = display;
            // The policy thread reacts to the new switches without waiting for
            // the save debounce.
            crate::power::request(crate::power::Event::Manual);
            self.mark_dirty();
        }
    }

    /// Console window and log file location.
    fn log_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("日志");
        ui.label(
            egui::RichText::new("运行日志既写入文件，也可以同时显示在一个实时窗口里。")
                .size(12.5)
                .weak(),
        );
        ui.add_space(12.0);

        let mut changed = false;
        let mut log = self.working.log.clone();

        egui::Frame::group(ui.style()).show(ui, |ui| {
            let mut console = log.console;
            if ui
                .checkbox(&mut console, "显示命令提示符（实时日志窗口）")
                .on_hover_text("随程序一起弹出，内容与日志文件一致；关闭则只写文件")
                .changed()
            {
                log.console = console;
                // Applied immediately: waiting for the save debounce would make
                // the checkbox feel broken.
                crate::console::sync(console);
                changed = true;
            }
            ui.label(
                egui::RichText::new("排查按键、压力阈值、刷新率切换时打开。")
                    .size(11.5)
                    .weak(),
            );
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("日志文件位置").strong());
            ui.label(
                egui::RichText::new(format!("当前写入：{}", crate::log::path().display()))
                    .size(11.5)
                    .weak(),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut log.directory)
                            .desired_width(340.0)
                            .hint_text(r"留空 = %LOCALAPPDATA%\MP14Tools"),
                    )
                    .changed();
                if ui.button("浏览…").clicked() {
                    if let Some(folder) = pick_folder() {
                        log.directory = folder;
                        changed = true;
                    }
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("使用默认位置").clicked() && !log.directory.is_empty() {
                    log.directory.clear();
                    changed = true;
                }
                if ui.button("打开日志目录").clicked() {
                    let directory = crate::log::path()
                        .parent()
                        .map(|parent| parent.display().to_string())
                        .unwrap_or_default();
                    open_in_explorer(&directory);
                }
                if ui.button("清空日志").clicked() {
                    crate::log::clear();
                    self.status = "已清空日志文件".to_string();
                    changed = true;
                }
            });

            ui.label(
                egui::RichText::new(
                    "改目录后立即生效，新目录里会新建 mp14tools.log；旧的日志文件不会被搬走，需要自己处理。",
                )
                .size(11.5)
                .weak(),
            );
        });

        if changed {
            self.working.log = log;
            self.mark_dirty();
        }
    }

    fn general_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("通用");

        let mut changed = false;
        let mut autostart_after: Option<bool> = None;

        ui.add_space(12.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            let mut autostart = self.autostart_enabled;
            if ui.checkbox(&mut autostart, "开机时自动启动").changed() {
                autostart_after = Some(autostart);
                changed = true;
            }

            changed |= ui
                .checkbox(&mut self.working.show_tray_icon, "显示托盘图标")
                .changed();
            changed |= ui
                .checkbox(&mut self.working.osd.enabled, "触发时显示 OSD 提示")
                .changed();

            if self.working.osd.enabled {
                let mut duration = self.working.osd.duration_ms as i32;
                if ui
                    .add(
                        egui::Slider::new(
                            &mut duration,
                            config::OSD_HOLD_RANGE.0 as i32..=config::OSD_HOLD_RANGE.1 as i32,
                        )
                        .step_by(500.0)
                        .text("OSD 停留时长 (ms)，之后 1 秒淡出"),
                    )
                    .changed()
                {
                    self.working.osd.duration_ms =
                        (duration as u32).clamp(config::OSD_HOLD_RANGE.0, config::OSD_HOLD_RANGE.1);
                    changed = true;
                }
            }
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("运行状态").strong());
            ui.add_space(4.0);
            status_row(
                ui,
                "触摸板监听",
                self.shared.touchpad_registered.load(Ordering::SeqCst),
            );
            status_row(ui, "WMI 事件订阅", self.shared.wmi_ready.load(Ordering::SeqCst));

            let last = self.shared.last_trigger();
            ui.label(
                egui::RichText::new(if last.is_empty() {
                    "最近触发：暂无".to_string()
                } else {
                    format!("最近触发：{last}")
                })
                .size(12.0),
            );

            // Privacy-free "someone else touched our settings" indicator: the
            // watcher reloads whatever the file now says and never writes back
            // over it, so the only thing left to do is say that it happened.
            let notice = self.shared.config_notice();
            if !notice.is_empty() {
                ui.label(
                    egui::RichText::new(format!("⚠ {notice}"))
                        .size(11.5)
                        .color(egui::Color32::from_rgb(0xD1, 0x74, 0x2B)),
                );
            }
        });

        ui.add_space(10.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(egui::RichText::new("文件").strong());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(config::config_path().display().to_string())
                    .size(11.5)
                    .weak(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("打开配置目录").clicked() {
                    open_in_explorer(&config::data_dir().display().to_string());
                }
                if ui.button("重新加载配置").clicked() {
                    self.sync_from_disk();
                }
            });
        });

        if changed {
            self.mark_dirty();
        }

        if let Some(enabled) = autostart_after {
            self.working.start_with_windows = enabled;
            self.autostart_enabled = enabled;
            self.apply_autostart(enabled);
        }
    }
}

impl eframe::App for SettingsApp {
    /// Runs before every `ui` pass, including while the window is hidden.
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.decorate_window(context);
        self.handle_shared_signals(context);

        // Closing the window only hides it: remapping keeps running from the
        // tray. The tray's "退出" entry is the one that really exits.
        if context.input(|input| input.viewport().close_requested())
            && !self.shared.exit_requested.load(Ordering::SeqCst)
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            context.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if let Some(since) = self.dirty_since {
            if since.elapsed() >= SAVE_DEBOUNCE {
                self.save();
            } else {
                context.request_repaint_after(SAVE_DEBOUNCE);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("navigation")
            .exact_size(196.0)
            .resizable(false)
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.tab {
                Tab::Touchpad => self.touchpad_tab(ui),
                Tab::OemKeys => self.oem_keys_tab(ui),
                Tab::Display => self.display_tab(ui),
                Tab::Log => self.log_tab(ui),
                Tab::General => self.general_tab(ui),
            });
        });

        // The live pressure readout needs a heartbeat, but only while a finger
        // is actually on the touchpad. A touchpad that keeps reporting idle
        // frames with zero pressure must not keep this loop awake, so the test
        // is "pressure above zero", not "a reading exists".
        if self.tab == Tab::Touchpad
            && self.shared.pressure().map(|value| value > 0).unwrap_or(false)
        {
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        }
    }
}

/// Modifier toggles plus a grouped target picker. Returns true when edited.
fn action_editor(ui: &mut egui::Ui, id: &str, action: &mut Action) -> bool {
    let mut changed = false;
    let mut target = action.target.clone();

    ui.horizontal(|ui| {
        ui.label("组合键");
        for (modifier_id, label, _) in catalog::MODIFIERS {
            let mut on = action
                .modifiers
                .iter()
                .any(|value| value.eq_ignore_ascii_case(modifier_id));
            if ui.toggle_value(&mut on, *label).changed() {
                action
                    .modifiers
                    .retain(|value| !value.eq_ignore_ascii_case(modifier_id));
                if on {
                    action.modifiers.push((*modifier_id).to_string());
                }
                changed = true;
            }
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("目标");
        let selected_text = target
            .as_deref()
            .map(catalog::label_of)
            .unwrap_or_else(|| "未设置".to_string());

        egui::ComboBox::from_id_salt(format!("{id}-target"))
            .selected_text(selected_text)
            .width(240.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(target.is_none(), "未设置").clicked() {
                    target = None;
                }

                ui.separator();
                ui.label(egui::RichText::new("鼠标").size(11.0).weak());
                for (mouse_id, mouse_label) in catalog::MOUSE_IDS {
                    let selected = is_selected(&target, mouse_id);
                    if ui.selectable_label(selected, *mouse_label).clicked() {
                        target = Some((*mouse_id).to_string());
                    }
                }

                for group in catalog::GROUPS {
                    let options: Vec<_> = catalog::key_options()
                        .iter()
                        .filter(|option| option.group == *group)
                        .collect();
                    if options.is_empty() {
                        continue;
                    }

                    ui.separator();
                    ui.label(egui::RichText::new(*group).size(11.0).weak());
                    egui::Grid::new(format!("{id}-{group}"))
                        .num_columns(6)
                        .spacing(egui::vec2(4.0, 4.0))
                        .show(ui, |ui| {
                            for (position, option) in options.iter().enumerate() {
                                let selected = is_selected(&target, option.id);
                                if ui.selectable_label(selected, option.label).clicked() {
                                    target = Some(option.id.to_string());
                                }
                                if (position + 1) % 6 == 0 {
                                    ui.end_row();
                                }
                            }
                        });
                }
            });
    });

    if target != action.target {
        action.target = target;
        changed = true;
    }

    changed
}

fn is_selected(target: &Option<String>, id: &str) -> bool {
    target
        .as_deref()
        .map(|value| value.eq_ignore_ascii_case(id))
        .unwrap_or(false)
}

/// Description of a slider's scale: the grid the value moves on plus the ticks
/// drawn under it.
///
/// Two very different scales are in use - a pressure threshold is a plain
/// integer, a haptic strength lives on the firmware's own 8-unit grid - so the
/// tick spacing is stated per scale instead of being derived from the step.
#[derive(Clone, Copy)]
struct Scale {
    min: i32,
    max: i32,
    /// Value change per slider step.
    step: i32,
    /// One tick every `tick` units.
    tick: i32,
    /// A full-length tick every `emphasis` units.
    emphasis: i32,
}

impl Scale {
    /// Pressure thresholds: integers, ruled every 25 with a long tick every 100,
    /// so a value can still be read off the scale at a glance.
    fn thresholds(min: i32, max: i32) -> Self {
        Self {
            min,
            max,
            step: 1,
            tick: 25,
            emphasis: 100,
        }
    }

    /// Haptic strengths: the firmware's own grid of 8, long tick every 40.
    fn haptics(min: i32, max: i32) -> Self {
        Self {
            min,
            max,
            step: config::HAPTIC_STRENGTH_STEP as i32,
            tick: config::HAPTIC_STRENGTH_STEP as i32,
            emphasis: 40,
        }
    }
}

/// One pressure threshold: an exact-value box with its own "确定" button, and a
/// full-width slider ruled with ticks.
///
/// Returns true only when the user confirmed the value, so nothing here can leak
/// a half-finished edit into the configuration.
fn threshold_editor(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    hint: &str,
    pending: &mut i32,
    applied: i32,
    scale: Scale,
) -> bool {
    let mut value = *pending;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.label(egui::RichText::new(hint).size(11.5).weak());
    });
    ui.add_space(6.0);

    let mut confirmed = false;
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut value)
                .range(scale.min..=scale.max)
                .speed(1.0),
        );
        ui.add_space(8.0);

        // Only worth pressing when the number differs from what is running.
        let edited = value != applied;
        if ui
            .add_enabled(edited, egui::Button::new("确定"))
            .on_hover_text("把左侧数值写入配置并立即生效")
            .clicked()
        {
            value = config::clamp_threshold(
                value as u16,
                scale.min as u16,
                scale.max as u16,
            ) as i32;
            confirmed = true;
        }

        ui.add_space(10.0);
        if edited {
            ui.label(
                egui::RichText::new(format!("未生效，当前为 {applied}"))
                    .size(11.5)
                    .color(egui::Color32::from_rgb(0xD1, 0x74, 0x2B)),
            );
        } else {
            ui.label(
                egui::RichText::new(format!("已生效：{applied}"))
                    .size(11.5)
                    .weak(),
            );
        }
    });

    ui.add_space(8.0);
    ticked_slider(ui, id, &mut value, scale);

    *pending = value;
    confirmed
}

/// A full-width slider ruled with ticks, so a value can be read off the scale
/// instead of guessed from where the handle happens to sit.
fn ticked_slider(ui: &mut egui::Ui, id: &str, value: &mut i32, scale: Scale) {
    let Scale {
        min,
        max,
        step,
        tick,
        emphasis,
    } = scale;

    ui.push_id(id, |ui| {
        // A taller rail leaves room for the ticks and gives the handle a size
        // that is actually comfortable to hit.
        ui.spacing_mut().interact_size.y = 26.0;
        ui.spacing_mut().slider_width = ui.available_width().max(240.0);

        let response = ui.add(
            egui::Slider::new(value, min..=max)
                .step_by(step as f64)
                .show_value(false),
        );

        // Same geometry egui derives for the rail and the handle, so every tick
        // sits exactly on the value it marks.
        let rect = response.rect;
        let track = rect.x_range().shrink(rect.height() / 2.5);
        let rail_bottom = rect.center().y + ui.spacing().slider_rail_height / 2.0;
        let length = rect.bottom() - rail_bottom;
        let color = ui.visuals().weak_text_color();

        let painter = ui.painter();
        for tick in (min..=max).step_by(tick.max(1) as usize) {
            let ratio = (tick - min) as f32 / (max - min).max(1) as f32;
            let x = egui::lerp(track, ratio);
            // The emphasised ticks carry the readable scale, so the in-between
            // ones only need half the length.
            let height = if emphasis > 0 && tick % emphasis == 0 {
                length
            } else {
                length * 0.5
            };
            painter.line_segment(
                [egui::pos2(x, rail_bottom), egui::pos2(x, rail_bottom + height)],
                egui::Stroke::new(1.0, color),
            );
        }
    });
}

/// Haptic ("vibration") strength of the touchpad.
///
/// Both values go straight to the firmware, so this group deliberately has no
/// "确定" button: the feedback has to be felt to be tuned, and the writer thread
/// collapses a whole drag into one sequence. The scale is the firmware's own -
/// see [`config::MAX_HAPTIC_STRENGTH`] for where 0..=128 and the step come from.
///
/// Returns true when edited.
fn haptics_editor(ui: &mut egui::Ui, haptics: &mut HapticsConfig, shared: &Arc<Shared>) -> bool {
    let mut changed = false;

    ui.label(egui::RichText::new("震动力度（触摸板硬件反馈）").strong());
    ui.label(
        egui::RichText::new(format!(
            "刻度 {}，范围 {}–{}，与触摸板固件一致；出厂值：轻触 {} / 重按 {}。",
            config::HAPTIC_STRENGTH_STEP,
            config::MIN_HAPTIC_STRENGTH,
            config::MAX_HAPTIC_STRENGTH,
            config::DEFAULT_NORMAL_STRENGTH,
            config::DEFAULT_DEEP_PRESS_STRENGTH,
        ))
        .size(11.5)
        .weak(),
    );
    ui.add_space(8.0);

    changed |= ui
        .checkbox(&mut haptics.enabled, "启用震动力度写入")
        .on_hover_text("关闭时不触碰触摸板固件，本工具保持只读")
        .changed();

    if !haptics.enabled {
        return changed;
    }

    ui.add_space(8.0);
    changed |= ui
        .checkbox(&mut haptics.factory_values, "使用出厂值")
        .on_hover_text("勾选后两个力度固定为出厂值，下面的滑条不再生效")
        .changed();

    if haptics.factory_values {
        haptics.normalize();
        return changed;
    }

    let min = config::MIN_HAPTIC_STRENGTH as i32;
    let max = config::MAX_HAPTIC_STRENGTH as i32;
    let scale = Scale::haptics(min, max);

    let mut normal = haptics.normal_strength as i32;
    let mut deep = haptics.deep_press_strength as i32;

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("轻触反馈").strong());
        changed |= ui
            .add(egui::DragValue::new(&mut normal).range(min..=max).speed(1.0))
            .changed();
        ui.label(
            egui::RichText::new("手指落在触摸板上时的反馈强度")
                .size(11.5)
                .weak(),
        );
    });
    ui.add_space(6.0);
    ticked_slider(ui, "haptic-normal", &mut normal, scale);

    ui.add_space(14.0);
    // The deep press is the stronger event, so its feedback never drops below
    // the light one - raising the light value therefore drags it along.
    deep = deep.max(normal);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("重按反馈").strong());
        changed |= ui
            .add(egui::DragValue::new(&mut deep).range(normal..=max).speed(1.0))
            .changed();
        ui.label(
            egui::RichText::new("重按被识别时的反馈强度，不低于轻触")
                .size(11.5)
                .weak(),
        );
    });
    ui.add_space(6.0);
    ticked_slider(
        ui,
        "haptic-deep",
        &mut deep,
        Scale {
            min: normal,
            ..scale
        },
    );

    ui.add_space(12.0);
    status_row(
        ui,
        "震动写入",
        shared
            .haptics_ready
            .load(std::sync::atomic::Ordering::SeqCst),
    );

    if shared.haptics_conflict() {
        ui.label(
            egui::RichText::new(
                "⚠ 触摸板已不再响应本工具的写入（可能被其他程序接管）。\
                 已暂停写入，保留当前硬件设置。",
            )
            .size(11.5)
            .color(egui::Color32::from_rgb(0xD1, 0x74, 0x2B)),
        );
    }

    let status = shared.haptics_status();
    if !status.is_empty() {
        ui.label(egui::RichText::new(status).size(11.5).weak());
    }

    if normal != haptics.normal_strength as i32 || deep != haptics.deep_press_strength as i32 {
        haptics.normal_strength = normal as u16;
        haptics.deep_press_strength = deep as u16;
        haptics.normalize();
        changed = true;
    }

    changed
}

fn pressure_readout(ui: &mut egui::Ui, shared: &Arc<Shared>) {
    let reading = shared.pressure();
    let (light, deep) = shared
        .config
        .read()
        .map(|config| {
            (
                config.touchpad.light_press_threshold as i32,
                config.touchpad.deep_press_threshold as i32,
            )
        })
        .unwrap_or((125, 500));

    ui.horizontal(|ui| {
        ui.label("当前压力");
        match reading {
            Some(pressure) => {
                ui.label(
                    egui::RichText::new(pressure.to_string())
                        .strong()
                        .color(if pressure >= deep {
                            egui::Color32::from_rgb(0x2E, 0x9E, 0x5B)
                        } else {
                            egui::Color32::from_gray(160)
                        }),
                );
                ui.label(
                    egui::RichText::new(format!("（轻按 {light} / 重按 {deep}）"))
                        .size(11.5)
                        .weak(),
                );

                // Shows the configured thresholds doing their job: the pressure
                // has to pass the light one before a press is tracked at all.
                let (state, color) = if pressure >= deep {
                    ("重按已触发", egui::Color32::from_rgb(0x2E, 0x9E, 0x5B))
                } else if pressure >= light {
                    ("轻按已触发", egui::Color32::from_rgb(0x1F, 0x6F, 0xEB))
                } else {
                    ("未达轻按阈值", egui::Color32::from_gray(150))
                };
                ui.label(egui::RichText::new(state).size(11.5).color(color));
            }
            None => {
                ui.label(egui::RichText::new("—").weak());
                ui.label(
                    egui::RichText::new("手指按住触摸板即可看到读数")
                        .size(11.5)
                        .weak(),
                );
            }
        }
    });

    // Only drawn while a finger is down: an always-present bar would keep the
    // widget animating (and the window repainting) for nothing.
    if let Some(pressure) = reading {
        let span = (deep as f32 * 1.2).max(1.0);
        let response = ui.add(
            egui::ProgressBar::new((pressure.max(0) as f32 / span).clamp(0.0, 1.0))
                .desired_width(ui.available_width().min(420.0))
                .desired_height(8.0),
        );

        // Mark where both thresholds sit on the bar.
        let rect = response.rect;
        for (threshold, color) in [
            (light as f32, egui::Color32::from_rgb(0x1F, 0x6F, 0xEB)),
            (deep as f32, egui::Color32::from_rgb(0x2E, 0x9E, 0x5B)),
        ] {
            let position = (threshold / span).clamp(0.0, 1.0);
            let x = egui::lerp(rect.x_range(), position);
            ui.painter().line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.5, color),
            );
        }
    }
}

fn status_row(ui: &mut egui::Ui, label: &str, ok: bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(if ok { "●" } else { "○" }).color(if ok {
                egui::Color32::from_rgb(0x2E, 0x9E, 0x5B)
            } else {
                egui::Color32::from_rgb(0xB0, 0x50, 0x50)
            }),
        );
        ui.label(egui::RichText::new(label).size(12.5));
    });
}

fn open_in_explorer(path: &str) {
    let operation = w!("open");
    let file = crate::win::wide(path);
    unsafe {
        let _ = windows::Win32::UI::Shell::ShellExecuteW(
            None,
            operation,
            PCWSTR(file.as_ptr()),
            None,
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
    }
}

/// Ask for a directory with the shell's own folder picker.
///
/// `None` when the user cancels or the shell refuses, so a failed pick can never
/// clear a path that already worked.
fn pick_folder() -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        SHBrowseForFolderW, SHGetPathFromIDListW, BIF_EDITBOX, BIF_NEWDIALOGSTYLE,
        BIF_RETURNONLYFSDIRS, BROWSEINFOW,
    };

    let title = crate::win::wide("选择文件夹");
    let mut display_name = [0u16; 260];

    let browse = BROWSEINFOW {
        hwndOwner: windows::Win32::Foundation::HWND(std::ptr::null_mut()),
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: windows::core::PWSTR(display_name.as_mut_ptr()),
        lpszTitle: PCWSTR(title.as_ptr()),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        lpfn: None,
        lParam: windows::Win32::Foundation::LPARAM(0),
        iImage: 0,
    };

    unsafe {
        let list = SHBrowseForFolderW(&browse);
        if list.is_null() {
            return None;
        }

        let mut buffer = [0u16; 260];
        let ok = SHGetPathFromIDListW(list, &mut buffer).as_bool();
        // The shell allocated the item id list with the task allocator.
        CoTaskMemFree(Some(list as *const core::ffi::c_void));

        if !ok {
            return None;
        }
        let path = crate::win::from_wide(&buffer);
        (!path.is_empty()).then_some(path)
    }
}

unsafe fn apply_dwm(
    window: windows::Win32::Foundation::HWND,
    attribute: DWMWINDOWATTRIBUTE,
    value: &impl Sized,
) {
    let _ = DwmSetWindowAttribute(
        window,
        attribute,
        value as *const _ as *const core::ffi::c_void,
        std::mem::size_of_val(value) as u32,
    );
}

/// Windows' own light/dark preference for apps.
fn system_uses_light_theme() -> bool {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };

    unsafe {
        let mut key = HKEY::default();
        let path = crate::win::wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        ) != windows::Win32::Foundation::ERROR_SUCCESS
        {
            return false;
        }

        let name = crate::win::wide("AppsUseLightTheme");
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        status == windows::Win32::Foundation::ERROR_SUCCESS && value != 0
    }
}

/// Prefer Segoe UI for Latin text and Microsoft YaHei for CJK, matching what the
/// rest of Windows 11 looks like. Falls back silently when neither is present.
fn install_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut installed = Vec::new();

    for (key, path, cjk) in [
        ("segoe", r"C:\Windows\Fonts\segoeui.ttf", false),
        ("cjk", r"C:\Windows\Fonts\msyh.ttc", true),
        ("cjk", r"C:\Windows\Fonts\Deng.ttf", true),
        ("cjk", r"C:\Windows\Fonts\simhei.ttf", true),
    ] {
        if fonts.font_data.contains_key(key) {
            continue;
        }

        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // Font collections need the right face index, and the face must actually
        // contain CJK glyphs or every Chinese label would render as boxes.
        let index = match pick_face(&bytes, cjk) {
            Some(index) => index,
            None => continue,
        };

        let mut data = egui::FontData::from_owned(bytes);
        data.index = index;
        fonts.font_data.insert(key.to_string(), Arc::new(data));
        installed.push(key);
    }

    if installed.is_empty() {
        return;
    }

    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    for key in installed.iter().rev() {
        proportional.insert(0, (*key).to_string());
    }

    context.set_fonts(fonts);
}

/// Face index inside a font file that covers the requested script.
fn pick_face(bytes: &[u8], needs_cjk: bool) -> Option<u32> {
    for index in 0..4u32 {
        let Ok(face) = ttf_parser::Face::parse(bytes, index) else {
            continue;
        };
        if !needs_cjk || face.glyph_index('按').is_some() {
            return Some(index);
        }
    }
    None
}

fn apply_visuals(context: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    visuals.panel_fill = if dark {
        egui::Color32::from_rgb(0x20, 0x20, 0x20)
    } else {
        egui::Color32::from_rgb(0xF3, 0xF3, 0xF3)
    };
    visuals.window_fill = visuals.panel_fill;
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.selection.bg_fill = egui::Color32::from_rgb(0x1F, 0x6F, 0xEB);

    context.set_visuals(visuals);
}

/// Run the settings window. Blocks until the window closes.
pub fn run(shared: Arc<Shared>) -> eframe::Result<()> {
    // The very same icon the tray uses: Windows takes the window icon for the
    // title bar, the taskbar button and Alt-Tab, so all three agree by
    // construction.
    let icon = egui::IconData {
        rgba: crate::icon::rgba(64),
        width: 64,
        height: 64,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_icon(icon)
            .with_inner_size([880.0, 620.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(move |context| Ok(Box::new(SettingsApp::new(context, shared)))),
    )
}
