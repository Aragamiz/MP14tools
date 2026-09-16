//! Display policy: drop the refresh rate on battery, and offer to turn HDR off.
//!
//! The behaviour mirrors the reference machine's "Smart Refresh Rate" tool,
//! reduced to what this tool needs: it reacts to power events instead of polling
//! (`WM_POWERBROADCAST` wakes us on both the AC/battery change and on resume),
//! and it asks before touching anything that would change what the user sees -
//! unless the configuration says to just do it.
//!
//! Per-monitor target modes can be imported from that tool's runtime folder
//! (`SRR/config.json`); without it the targets are derived from what the panel
//! reports as available at the current resolution.
//!
//! Everything runs on one thread that blocks on a channel, so the tool costs
//! nothing while the power state does not change.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::config::{BatteryAction, DisplayConfig, ExternalRate};
use crate::display::Display;
use crate::notice::{self, Choice, Notice};
use crate::state::Shared;

/// What woke the policy up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// AC <-> battery changed.
    PowerSource,
    /// Back from sleep.
    Resume,
    /// The user pressed "check now" in the settings window.
    Manual,
}

/// Bursts are common (a resume usually carries a power-status change too), so
/// events arriving within this window are handled once.
const COALESCE: Duration = Duration::from_millis(400);

static EVENTS: OnceLock<Sender<Event>> = OnceLock::new();

/// Rate each display had before this tool lowered it.
///
/// Deliberately in memory only: after a restart there is nothing to restore, and
/// the registry still holds whatever the user had chosen.
static REMEMBERED: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();

fn remembered() -> &'static Mutex<HashMap<String, u32>> {
    REMEMBERED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start the policy thread.
pub fn spawn(shared: Arc<Shared>) {
    // Log what the detection sees, once. Without this the only way to find out
    // which display is considered internal - or whether HDR was noticed at all -
    // would be to change the display and watch what happens.
    log_inventory(&shared);

    let (sender, receiver) = mpsc::channel::<Event>();
    if EVENTS.set(sender).is_err() {
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("display".to_string())
        .spawn(move || worker(shared, receiver));

    if let Err(error) = spawned {
        crate::log::line(&format!("display: thread spawn failed: {error}"));
    }
}

fn log_inventory(shared: &Arc<Shared>) {
    let enabled = shared
        .config
        .read()
        .map(|config| config.display.enabled)
        .unwrap_or(false);
    if !enabled {
        return;
    }

    match crate::display::on_battery() {
        Some(true) => crate::log::line("display: on battery"),
        Some(false) => crate::log::line("display: on AC"),
        None => crate::log::line("display: no battery information"),
    }

    for display in crate::display::list() {
        crate::log::line(&format!(
            "display: {} [{}] {}x{} @{} Hz, HDR {} ({})",
            display.adapter,
            if display.internal { "internal" } else { "external" },
            display.width,
            display.height,
            display.refresh,
            if display.hdr_enabled { "on" } else { "off" },
            if display.hdr_supported {
                "supported"
            } else {
                "unsupported"
            },
        ));
    }
}

/// Ask the policy thread to look at the current power state.
pub fn request(event: Event) {
    if let Some(sender) = EVENTS.get() {
        let _ = sender.send(event);
    }
}

fn worker(shared: Arc<Shared>, receiver: Receiver<Event>) {
    loop {
        let Ok(event) = receiver.recv() else {
            return;
        };

        // Coalesce a burst, but never let a resume be swallowed by the power
        // change that accompanies it - the HDR check hangs off the resume.
        let mut event = event;
        loop {
            match receiver.recv_timeout(COALESCE) {
                Ok(newer) => {
                    if newer == Event::Resume {
                        event = Event::Resume;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        handle(&shared, event);
    }
}

/// One step a notice can offer.
#[derive(Clone, Copy)]
enum Step {
    SwitchRate,
    UndoRate,
    DisableHdr,
}

fn handle(shared: &Arc<Shared>, event: Event) {
    let config = match shared.config.read() {
        Ok(config) => config.display.clone(),
        Err(_) => return,
    };

    if !config.enabled {
        if event == Event::Manual {
            shared.set_display_status("显示调节未启用".to_string());
        }
        return;
    }

    // The topology may differ from the cached one after a resume or a dock
    // change, so every event starts from a fresh enumeration.
    crate::display::invalidate();

    let on_battery = crate::display::on_battery();
    let displays = targets(&config);

    if on_battery == Some(false) {
        restore(shared, &config);
        return;
    }

    if displays.is_empty() {
        if event == Event::Manual {
            shared.set_display_status("没有符合范围设置的显示器".to_string());
        }
        return;
    }

    if event == Event::Resume {
        // The rate is already where it belongs; only HDR is re-checked.
        check_hdr(shared, &config, &displays);
        return;
    }

    enter_battery(shared, &config, &displays);
}

/// Displays the configuration allows this tool to touch.
fn targets(config: &DisplayConfig) -> Vec<Display> {
    crate::display::list()
        .into_iter()
        .filter(|display| {
            if display.internal {
                config.internal
            } else {
                config.external
            }
        })
        .collect()
}

/// Mode a display is switched to on battery, or `None` when the configuration
/// asks for something this display cannot report.
///
/// The built-in panel has exactly two modes to choose from (60 or 120 Hz) and
/// lands on the highest one not above the choice; an external display is either
/// driven at its highest mode or at 60 Hz.
fn target_rate(config: &DisplayConfig, display: &Display) -> Option<u32> {
    if display.internal {
        crate::display::rate_at_most(
            &display.adapter,
            display.width,
            display.height,
            config.internal_refresh_rate,
        )
    } else {
        match config.external_refresh_rate {
            ExternalRate::Highest => {
                crate::display::highest_rate(&display.adapter, display.width, display.height)
            }
            ExternalRate::Hz60 => {
                crate::display::rate_at_most(&display.adapter, display.width, display.height, 60)
            }
        }
    }
}

/// Displays whose HDR should be offered for switching off.
///
/// HDR is only handled for the built-in panel: it is the display that actually
/// drains the battery, and turning it off there is the whole point of the check.
fn hdr_candidates(config: &DisplayConfig, displays: &[Display]) -> Vec<Display> {
    if !config.hdr_check {
        return Vec::new();
    }

    displays
        .iter()
        .filter(|display| display.internal && display.hdr_supported && display.hdr_enabled)
        .cloned()
        .collect()
}

/// Button label for the rate step: name the rate when every display ends up on
/// the same one, and stay generic when they do not.
fn switch_label(plan: &[(Display, u32)]) -> String {
    match plan.first() {
        Some((_, first)) if plan.iter().all(|(_, target)| target == first) => {
            format!("切换到 {first} Hz")
        }
        _ => "切换刷新率".to_string(),
    }
}

/// AC came back: put every rate this tool lowered back where it was.
fn restore(shared: &Arc<Shared>, config: &DisplayConfig) {
    let pending: Vec<(String, u32)> = match remembered().lock() {
        Ok(mut remembered) => remembered.drain().collect(),
        Err(_) => return,
    };

    if pending.is_empty() {
        return;
    }

    // Displays outside the configured range keep the rate they have; forget
    // them without touching them.
    let allowed: Vec<String> = targets(config)
        .into_iter()
        .map(|display| display.adapter)
        .collect();

    for (adapter, rate) in pending {
        if !allowed.iter().any(|name| name == &adapter) {
            continue;
        }

        match crate::display::set_rate(&adapter, rate) {
            Ok(()) => crate::log::line(&format!("display: {adapter} restored to {rate} Hz")),
            Err(error) => crate::log::line(&format!("display: restore failed: {error}")),
        }
        shared.set_display_status(format!("已恢复 {adapter} 到 {rate} Hz"));
    }

    crate::display::invalidate();
}

/// Battery, from a power change or a manual check.
fn enter_battery(shared: &Arc<Shared>, config: &DisplayConfig, displays: &[Display]) {
    let hdr_displays = hdr_candidates(config, displays);

    // Displays that are not already on the mode the configuration asks for.
    let plan: Vec<(Display, u32)> = displays
        .iter()
        .filter_map(|display| {
            let target = target_rate(config, display)?;
            (display.refresh != target).then(|| (display.clone(), target))
        })
        .collect();

    if plan.is_empty() && hdr_displays.is_empty() {
        crate::log::line("display: battery, nothing to do");
        shared.set_display_status("已是设定档位，无需调整".to_string());
        return;
    }

    let mut steps: Vec<(String, Step)> = Vec::new();
    match config.battery_action {
        BatteryAction::Force => {
            for (display, target) in &plan {
                if let Err(error) = apply_rate(shared, display, *target) {
                    crate::log::line(&format!("display: {error}"));
                }
            }
            if !plan.is_empty() {
                steps.push(("撤销".to_string(), Step::UndoRate));
            }
        }
        BatteryAction::Notify => {
            if !plan.is_empty() {
                steps.push((switch_label(&plan), Step::SwitchRate));
            }
        }
        BatteryAction::Off => return,
    }

    if !hdr_displays.is_empty() {
        steps.push(("关闭 HDR".to_string(), Step::DisableHdr));
    }

    if steps.is_empty() {
        return;
    }

    let mut notice = Notice::new(describe(&plan, &hdr_displays), steps[0].0.clone());
    if let Some((label, _)) = steps.get(1) {
        notice = notice.with_second(label.clone());
    }

    let chosen = match notice::ask(notice) {
        Choice::First => steps.first().map(|(_, step)| *step),
        Choice::Second => steps.get(1).map(|(_, step)| *step),
        Choice::Dismissed => None,
    };

    match chosen {
        Some(Step::SwitchRate) => {
            for (display, target) in &plan {
                if let Err(error) = apply_rate(shared, display, *target) {
                    crate::log::line(&format!("display: {error}"));
                }
            }
        }
        Some(Step::UndoRate) => restore(shared, config),
        Some(Step::DisableHdr) => turn_off_hdr(shared, &hdr_displays),
        None => {}
    }
}

/// Wake-up in battery mode: offer to turn HDR off again.
fn check_hdr(shared: &Arc<Shared>, config: &DisplayConfig, displays: &[Display]) {
    let hdr_displays = hdr_candidates(config, displays);

    if hdr_displays.is_empty() {
        return;
    }

    crate::log::line(&format!(
        "display: HDR is on while on battery ({}), asking",
        hdr_displays
            .iter()
            .map(|display| display.adapter.clone())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let names = hdr_displays
        .iter()
        .map(|display| display.label.clone())
        .collect::<Vec<_>>()
        .join("、");

    let notice = Notice::new(
        format!("电池供电时内屏 HDR 已开启（{names}）。关闭它可以明显省电。"),
        "关闭 HDR",
    );

    if notice::ask(notice) == Choice::First {
        turn_off_hdr(shared, &hdr_displays);
    }
}

/// Switch one display to `target`, remembering where it was.
fn apply_rate(shared: &Arc<Shared>, display: &Display, target: u32) -> Result<(), String> {
    let previous = crate::display::current_rate(&display.adapter).unwrap_or(display.refresh);

    crate::display::set_rate(&display.adapter, target)?;

    if let Ok(mut remembered) = remembered().lock() {
        remembered.insert(display.adapter.clone(), previous);
    }

    crate::log::line(&format!(
        "display: {} switched {previous} -> {target} Hz",
        display.adapter
    ));
    shared.set_display_status(format!("{} {previous} → {target} Hz", display.adapter));
    crate::display::invalidate();
    Ok(())
}

/// Turn HDR off on the given displays.
fn turn_off_hdr(shared: &Arc<Shared>, displays: &[Display]) {
    for display in displays {
        match crate::display::set_hdr(display, false) {
            Ok(()) => {
                crate::log::line(&format!("display: HDR off on {}", display.adapter));
                shared.set_display_status(format!("{} 已关闭 HDR", display.adapter));
            }
            Err(error) => crate::log::line(&format!("display: {error}")),
        }
    }
    crate::display::invalidate();
}

/// Sentence shown in the notice.
fn describe(plan: &[(Display, u32)], hdr: &[Display]) -> String {
    let mut text = String::from("已切换到电池供电。");

    if plan.is_empty() {
        text.push_str("刷新率已是设定档位。");
    }
    for (display, target) in plan {
        text.push_str(&format!(
            "{}{} {} Hz → {} Hz。",
            if display.internal { "内屏 " } else { "外屏 " },
            display.label,
            display.refresh,
            target
        ));
    }

    if !hdr.is_empty() {
        text.push_str(" 另外内屏 HDR 正在开启。");
    }

    text
}
