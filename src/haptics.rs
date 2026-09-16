//! Touchpad haptic ("vibration") strength, written through the vendor's private
//! HID collection.
//!
//! # Why this module exists
//!
//! A haptic touchpad such as this one exposes a *second* HID collection next to
//! the standard precision-touchpad one. That collection is not part of any
//! public contract; it answers a small framed protocol that the vendor's own
//! settings application uses, and it is where the feedback strength lives:
//!
//! ```text
//! byte 0   report id        0x0D
//! byte 1   payload length
//! byte 2   checksum         (XOR of the payload) + 1
//! byte 3.. payload
//! ```
//!
//! The reports are a fixed 33 bytes. A change is never a single message: the
//! firmware expects a sequence - initialise, configure, commit, unlock - with a
//! short pause between the packets, because it has to digest each one before the
//! next arrives.
//!
//! The two strengths are one byte each in the configuration payload. The scale
//! they live on - `0..=128`, step `8` - comes from the values the firmware and
//! the vendor tooling actually use (see [`crate::config::MAX_HAPTIC_STRENGTH`]),
//! not from a number chosen here.
//!
//! # Safety of the write
//!
//! Writes are off unless [`HapticsConfig::enabled`] is set, they carry only the
//! two strength bytes, and both default to the factory values - so enabling the
//! feature does not by itself change how the touchpad feels. Everything else the
//! tool does remains read-only.
//!
//! [`HapticsConfig::enabled`]: crate::config::HapticsConfig::enabled

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::{HidD_GetInputReport, HidD_SetOutputReport};
use windows::Win32::Foundation::{CloseHandle, HANDLE, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::UI::Input::{
    GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICENAME, RIM_TYPEHID,
};

use crate::notice::Notice;
use crate::state::Shared;

/// Report id of the private protocol.
const REPORT_ID: u8 = 0x0D;
/// Fixed length of every report of that protocol.
const REPORT_LENGTH: usize = 33;
/// How often the device is asked whether it still holds our values.
const CHECK_INTERVAL: Duration = Duration::from_secs(120);
/// Command byte that carries the two strengths.
const COMMAND_VIBRATION: u8 = 0x5D;
/// Commit id belonging to [`COMMAND_VIBRATION`].
const COMMIT_VIBRATION: u8 = 0xA3;
/// Data length of the unlock packet of [`COMMAND_VIBRATION`].
const UNLOCK_VIBRATION: u8 = 0x04;
/// Pause between two packets; the firmware needs time to digest each one.
const INTER_PACKET_DELAY: Duration = Duration::from_millis(130);
/// A slider drag produces a change per frame. Waiting this long before writing
/// collapses a whole drag into a single sequence.
const DEBOUNCE: Duration = Duration::from_millis(220);
/// How many discovered device paths to log when the marker does not match.
const LOGGED_CANDIDATES: usize = 8;

/// One request to write, as handed to the worker thread. Moves through the
/// channel, so it deliberately has no `Clone`.
struct Request {
    marker: String,
    normal: u16,
    deep: u16,
}

static REQUESTS: OnceLock<Sender<Request>> = OnceLock::new();

/// Start the writer thread and apply the configured strengths once.
pub fn spawn(shared: Arc<Shared>) {
    let (sender, receiver) = mpsc::channel::<Request>();
    if REQUESTS.set(sender).is_err() {
        return;
    }

    let spawned = std::thread::Builder::new()
        .name("haptics".to_string())
        .spawn(move || worker(shared, receiver));

    if let Err(error) = spawned {
        crate::log::line(&format!("haptics: thread spawn failed: {error}"));
    }
}

/// Queue the strengths currently in `shared`, if the feature is on.
///
/// Called from the startup path, from the settings window after a save and from
/// the configuration watcher, so a hand-edited file and a slider both end up in
/// the same place.
pub fn request_from(shared: &Arc<Shared>) {
    let Ok(config) = shared.config.read() else {
        return;
    };
    if !config.haptics.enabled {
        return;
    }

    let request = Request {
        marker: config.haptics.device_marker.clone(),
        normal: config.haptics.normal_strength,
        deep: config.haptics.deep_press_strength,
    };
    drop(config);

    if let Some(sender) = REQUESTS.get() {
        let _ = sender.send(request);
    }
}

fn worker(shared: Arc<Shared>, receiver: Receiver<Request>) {
    // The configured values are in effect from the first moment the tool runs.
    request_from(&shared);

    loop {
        // The timeout is the only reason this thread wakes up on its own: it is
        // how often the device is asked whether it still holds our values.
        let request = match receiver.recv_timeout(CHECK_INTERVAL) {
            Ok(request) => request,
            Err(RecvTimeoutError::Timeout) => {
                verify(&shared);
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let mut request = request;

        // Coalesce: during a drag only the newest value matters, and every write
        // costs ~0.5 s in the firmware sequencing below.
        loop {
            match receiver.recv_timeout(DEBOUNCE) {
                Ok(newer) => request = newer,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        // While the device is not answering there is nothing to write to; the
        // switch in the settings window is what ends this state.
        if shared.haptics_conflict() {
            continue;
        }

        report(&shared, write(&request), &request);
        verify(&shared);
    }
}

/// Ask the device whether it is still ours to write to.
///
/// # What this can and cannot detect
///
/// The private protocol has no way to read the two strengths back: the frame the
/// collection answers with carries no echo of the configuration (dumping it
/// after a write shows an unrelated 4-byte payload), so a silent change made by
/// another program is invisible to us. What *is* observable is a program that
/// takes the collection over or resets it: that makes the device stop answering.
///
/// So this check is about reachability, and it is deliberately honest about being
/// nothing more: when a device that answered before stops answering, the tool
/// stops writing and says so instead of quietly re-applying its values.
fn verify(shared: &Arc<Shared>) {
    let marker = match shared.config.read() {
        Ok(config) => {
            if !config.haptics.enabled {
                return;
            }
            config.haptics.device_marker.clone()
        }
        Err(_) => return,
    };

    match read_state(&marker) {
        Ok(_) => {
            if shared.haptics_conflict() {
                crate::log::line("haptics: the device answers again");
                shared.set_haptics_conflict(false);
                shared.set_haptics_status(true, "与设备通信正常".to_string());
            }
        }
        Err(error) => {
            // Only a conflict once it worked: a device that was never reachable
            // is simply a machine without this touchpad.
            if !shared.haptics_ready() || shared.haptics_conflict() {
                return;
            }

            crate::log::line(&format!("haptics: the device stopped answering ({error})"));
            shared.set_haptics_conflict(true);
            shared.set_haptics_status(false, format!("设备无响应：{error}"));

            let notice = Notice::new(
                "触摸板震动设置已无法写入：该设备不再响应，可能是其他程序接管了它。\
                 本工具已暂停写入，改为保留现状。",
                "知道了",
            );
            crate::notice::ask(notice);
        }
    }
}

/// Mirror the outcome into the log, the log file and the settings window.
fn report(shared: &Arc<Shared>, outcome: Result<(), String>, request: &Request) {
    match outcome {
        Ok(()) => {
            crate::log::line(&format!(
                "haptics: written, 轻触={} 重按={}",
                request.normal, request.deep
            ));
            shared.set_haptics_status(true, format!("{} / {}", request.normal, request.deep));
        }
        Err(error) => {
            crate::log::line(&format!("haptics: unavailable ({error})"));
            shared.set_haptics_status(false, error);
        }
    }
}

/// Read the frame the firmware answers with.
///
/// The device echoes the settings it currently holds, which is the only way to
/// notice that something else changed them: the private protocol has no other
/// read path.
fn read_state(marker: &str) -> Result<[u8; REPORT_LENGTH], String> {
    let path = device_path(marker)?;
    let path = crate::win::wide(&path);

    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(|error| format!("CreateFileW failed: {error}"))?;

    let outcome = read_from(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    outcome
}

fn read_from(handle: HANDLE) -> Result<[u8; REPORT_LENGTH], String> {
    // The handshake first: the collection only answers once it has been opened.
    let init = initialize();
    let _ = unsafe {
        HidD_SetOutputReport(handle, init.as_ptr().cast::<c_void>(), init.len() as u32)
    };
    std::thread::sleep(INTER_PACKET_DELAY);

    let mut state = [0u8; REPORT_LENGTH];
    state[0] = REPORT_ID;

    if !unsafe {
        HidD_GetInputReport(handle, state.as_mut_ptr().cast::<c_void>(), REPORT_LENGTH as u32)
    } {
        return Err("HidD_GetInputReport failed".to_string());
    }

    Ok(state)
}

/// Open the private collection and push the whole sequence to it.
fn write(request: &Request) -> Result<(), String> {
    let path = device_path(&request.marker)?;
    let path = crate::win::wide(&path);

    let handle = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(|error| format!("CreateFileW failed: {error}"))?;

    let outcome = send(handle, request);
    unsafe {
        let _ = CloseHandle(handle);
    }
    outcome
}

/// Write the four packets the firmware expects, in order.
fn send(handle: HANDLE, request: &Request) -> Result<(), String> {
    let payload = [
        request.normal as u8,
        0x00,
        request.deep as u8,
        0x00,
    ];

    let packets = [
        initialize(),
        configuration(COMMAND_VIBRATION, &payload),
        commit(COMMAND_VIBRATION, COMMIT_VIBRATION),
        unlock(COMMAND_VIBRATION, UNLOCK_VIBRATION),
    ];

    for (index, packet) in packets.iter().enumerate() {
        let written = unsafe {
            HidD_SetOutputReport(handle, packet.as_ptr().cast::<c_void>(), packet.len() as u32)
        };
        if !written {
            return Err(format!(
                "HidD_SetOutputReport failed on packet {} ({})",
                index + 1,
                hex(packet)
            ));
        }

        // The last pause is harmless and keeps the loop uniform.
        std::thread::sleep(INTER_PACKET_DELAY);
    }

    Ok(())
}

/// Handshake packet, sent before every sequence.
fn initialize() -> [u8; REPORT_LENGTH] {
    let mut packet = [0u8; REPORT_LENGTH];
    packet[..9].copy_from_slice(&[REPORT_ID, 0x07, 0x16, 0x00, 0x00, 0x01, 0x00, 0x04, 0x10]);
    packet
}

/// A configuration packet: `payload` plus the six-byte command header.
fn configuration(command: u8, payload: &[u8]) -> [u8; REPORT_LENGTH] {
    let mut core = vec![0u8; payload.len() + 6];
    core[2] = (payload.len() + 1) as u8;
    core[5] = command;
    core[6..].copy_from_slice(payload);

    frame(&core, (payload.len() + 7) as u8)
}

/// The packet that makes the firmware accept the configuration.
fn commit(command: u8, commit_id: u8) -> [u8; REPORT_LENGTH] {
    let core = [0x01, 0x00, 0x03, 0x00, 0x00, 0x01, command, commit_id];
    frame(&core, 0x09)
}

/// The packet that leaves configuration mode again.
fn unlock(command: u8, data_length: u8) -> [u8; REPORT_LENGTH] {
    let core = [0x01, 0x00, 0x01, 0x00, data_length, command];
    frame(&core, 0x07)
}

/// Wrap a payload in the report framing: id, length, checksum.
fn frame(core: &[u8], payload_length: u8) -> [u8; REPORT_LENGTH] {
    let mut packet = [0u8; REPORT_LENGTH];
    packet[0] = REPORT_ID;
    packet[1] = payload_length;
    packet[2] = checksum(core);

    let count = core.len().min(REPORT_LENGTH - 3);
    packet[3..3 + count].copy_from_slice(&core[..count]);
    packet
}

/// XOR of the payload, incremented - the firmware's own idea of a checksum.
fn checksum(core: &[u8]) -> u8 {
    core.iter()
        .fold(0u8, |value, byte| value ^ byte)
        .wrapping_add(1)
}

/// `01-02-03` style dump, for the log.
fn hex(packet: &[u8]) -> String {
    packet
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join("-")
}

/// Interface path of the private HID collection.
///
/// The list of raw input devices already contains every HID collection with
/// input reports, which includes the private one, so no device enumeration
/// through SetupAPI is needed.
fn device_path(marker: &str) -> Result<String, String> {
    let marker = marker.to_ascii_lowercase();
    let entry_size = size_of::<RAWINPUTDEVICELIST>() as u32;

    let mut count = 0u32;
    if unsafe { GetRawInputDeviceList(None, &mut count, entry_size) } == u32::MAX {
        return Err("GetRawInputDeviceList failed".to_string());
    }
    if count == 0 {
        return Err("no raw input devices".to_string());
    }

    // Zeroed is the documented starting state for this list; the call fills it.
    let mut devices = vec![unsafe { zeroed::<RAWINPUTDEVICELIST>() }; count as usize];
    let listed = unsafe { GetRawInputDeviceList(Some(devices.as_mut_ptr()), &mut count, entry_size) };
    if listed == u32::MAX {
        return Err("GetRawInputDeviceList failed on the second call".to_string());
    }
    devices.truncate(listed as usize);

    let mut candidates = Vec::new();
    for device in devices.iter().filter(|device| device.dwType == RIM_TYPEHID) {
        let Some(path) = device_name(device.hDevice) else {
            continue;
        };

        // The interface path is the only thing that identifies the private
        // collection: it carries the vendor id and the collection number.
        if path.to_ascii_lowercase().contains(&marker) {
            return Ok(path);
        }
        candidates.push(path);
    }

    // Nothing matched, which is the one failure a user can fix by hand - so list
    // what was found instead of only reporting that it failed.
    let mut message = format!("no HID device matches \"{marker}\"");
    if candidates.is_empty() {
        message.push_str(" (no HID device paths were readable)");
    } else {
        message.push_str("; saw: ");
        message.push_str(
            &candidates
                .iter()
                .take(LOGGED_CANDIDATES)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    Err(message)
}

/// Interface path behind a raw input device handle.
fn device_name(handle: HANDLE) -> Option<String> {
    // A null buffer asks for the length instead of the name.
    let mut size = 0u32;
    let required = unsafe {
        GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICENAME, None, &mut size)
    };
    if required == u32::MAX || size == 0 {
        return None;
    }

    // For `RIDI_DEVICENAME` both counts are in UTF-16 units, not bytes.
    let mut buffer = vec![0u16; size as usize + 1];
    let mut size = buffer.len() as u32;
    let written = unsafe {
        GetRawInputDeviceInfoW(
            Some(handle),
            RIDI_DEVICENAME,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            &mut size,
        )
    };
    if written == u32::MAX {
        return None;
    }

    buffer.truncate((written as usize).min(buffer.len()));
    Some(crate::win::from_wide(&buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A null-padded report with `prefix` at the start.
    fn report(prefix: &[u8]) -> [u8; REPORT_LENGTH] {
        let mut packet = [0u8; REPORT_LENGTH];
        packet[..prefix.len()].copy_from_slice(prefix);
        packet
    }

    /// These two packets are byte-for-byte copies of the ones the reference
    /// implementation sends in its haptic on/off sequence. They pin down the
    /// framing *and* the checksum: the first is an unlock packet, the second a
    /// commit packet, and they are the only two shapes shared between that
    /// sequence and the vibration one.
    #[test]
    fn framing_matches_the_reference_packets() {
        assert_eq!(
            unlock(0x59, 0x02),
            report(&[0x0D, 0x07, 0x5C, 0x01, 0x00, 0x01, 0x00, 0x02, 0x59])
        );
        assert_eq!(
            commit(0x59, 0xA7),
            report(&[0x0D, 0x09, 0xFE, 0x01, 0x00, 0x03, 0x00, 0x00, 0x01, 0x59, 0xA7])
        );
    }

    /// The strength payload is two bytes with a zero separator after each, the
    /// length byte counts the payload rather than the report, and everything
    /// behind it stays zero.
    #[test]
    fn vibration_payload_is_framed() {
        let packet = configuration(COMMAND_VIBRATION, &[80, 0x00, 104, 0x00]);

        assert_eq!(packet[0], REPORT_ID);
        assert_eq!(packet[1], 11);
        // (XOR of 00 00 05 00 00 5D 50 00 68 00) + 1
        assert_eq!(packet[2], 0x61);
        assert_eq!(
            &packet[3..13],
            &[0x00, 0x00, 0x05, 0x00, 0x00, 0x5D, 80, 0x00, 104, 0x00]
        );
        assert!(packet[13..].iter().all(|byte| *byte == 0));
    }

    /// The scale the settings window offers has to stay on the grid the firmware
    /// writes, otherwise a slider position would round to something else.
    #[test]
    fn every_documented_strength_sits_on_the_grid() {
        for value in [
            crate::config::DEFAULT_NORMAL_STRENGTH,
            crate::config::DEFAULT_DEEP_PRESS_STRENGTH,
            crate::config::MAX_HAPTIC_STRENGTH,
            0,
            56,
        ] {
            assert_eq!(crate::config::snap_strength(value), value);
        }
    }
}
