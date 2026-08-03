#![cfg(target_os = "macos")]

//! Native macOS keyboard capture via a CGEventTap.
//!
//! `rdev`'s macOS event-tap callback calls HIToolbox TextServices
//! (`TSMGetInputSourceProperty`) off the main thread to build key names. On
//! modern macOS this asserts (`_dispatch_assert_queue_fail`) and traps
//! (`SIGTRAP`) on the first captured key, killing the process. This module
//! reads only the keycode — no TextServices, no trap — and routes mapped keys
//! to the broker with suppression, exactly as `rdev::grab` does on other
//! platforms.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};
use tokio::sync::mpsc;
use usahp_core::{Action, InputKind, Mapping};

use crate::broker::{BrokerCommand, PhysicalEvent};

/// `kCGKeyboardEventKeycode`
const KEYCODE_FIELD: u32 = 9;

/// usahp key-name → macOS virtual keycode (mirrors `parse_key`'s accepted names).
fn name_to_keycode() -> HashMap<&'static str, u16> {
    let raw: &[(&str, u16)] = &[
        ("space", 49),
        ("return", 36),
        ("enter", 36),
        ("escape", 53),
        ("esc", 53),
        ("tab", 48),
        ("up", 126),
        ("uparrow", 126),
        ("down", 125),
        ("downarrow", 125),
        ("left", 123),
        ("leftarrow", 123),
        ("right", 124),
        ("rightarrow", 124),
        ("a", 0),
        ("b", 11),
        ("c", 8),
        ("d", 2),
        ("e", 14),
        ("f", 3),
        ("g", 5),
        ("h", 4),
        ("i", 34),
        ("j", 38),
        ("k", 40),
        ("l", 37),
        ("m", 46),
        ("n", 45),
        ("o", 31),
        ("p", 35),
        ("q", 12),
        ("r", 15),
        ("s", 1),
        ("t", 17),
        ("u", 32),
        ("v", 9),
        ("w", 13),
        ("x", 7),
        ("y", 16),
        ("z", 6),
    ];
    raw.iter().copied().collect()
}

/// Install a session CGEventTap that turns the configured keyboard mappings
/// into broker `Input` commands and suppresses the captured keys. Requires
/// macOS Accessibility permission; logs and returns otherwise.
pub fn spawn(mappings: &[Mapping], broker: mpsc::Sender<BrokerCommand>, capture: Arc<AtomicBool>) {
    let names = name_to_keycode();
    let mut by_key: HashMap<u16, Vec<String>> = HashMap::new();
    for m in mappings.iter().filter(|m| m.input == InputKind::Keyboard) {
        if let Some(&kc) = names.get(m.code.to_ascii_lowercase().as_str()) {
            by_key.entry(kc).or_default().push(m.id.clone());
        }
    }
    if by_key.is_empty() {
        tracing::warn!("no capturable keyboard mappings; macOS tap not installed");
        return;
    }
    let by_key = Arc::new(by_key);

    if let Err(error) = std::thread::Builder::new()
        .name("usahp-macos-tap".into())
        .spawn(move || {
            tracing::info!(
                "installing macOS CGEventTap for {} keycode(s) — needs Accessibility",
                by_key.len()
            );
            let installed = CGEventTap::with_enabled(
                CGEventTapLocation::Session,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                vec![CGEventType::KeyDown, CGEventType::KeyUp],
                move |_proxy, event_type, event| {
                    // Capture released: pass the event through untouched.
                    if !capture.load(Ordering::Relaxed) {
                        return CallbackResult::Keep;
                    }
                    let keycode = event.get_integer_value_field(KEYCODE_FIELD) as u16;
                    let action = match event_type {
                        CGEventType::KeyDown => Action::Pressed,
                        CGEventType::KeyUp => Action::Released,
                        _ => return CallbackResult::Keep,
                    };
                    if let Some(ids) = by_key.get(&keycode) {
                        for id in ids {
                            if broker
                                .blocking_send(BrokerCommand::Input(PhysicalEvent {
                                    mapping_id: id.clone(),
                                    action,
                                    confidence: Some(if action == Action::Pressed {
                                        100.0
                                    } else {
                                        0.0
                                    }),
                                }))
                                .is_err()
                            {
                                break;
                            }
                        }
                        CallbackResult::Drop // suppress: key consumed by USAHP
                    } else {
                        CallbackResult::Keep
                    }
                },
                CFRunLoop::run_current,
            );
            if installed.is_err() {
                tracing::error!(
                    "CGEventTap install failed — grant Accessibility to this process \
                     (System Settings → Privacy → Accessibility) and restart"
                );
            }
        })
    {
        tracing::error!(%error, "could not spawn macOS tap thread");
    }
}

#[cfg(test)]
mod tests {
    use super::name_to_keycode;

    #[test]
    fn maps_every_documented_keyboard_code() {
        let keycodes = name_to_keycode();
        let expected = [
            ("space", 49),
            ("return", 36),
            ("escape", 53),
            ("tab", 48),
            ("up", 126),
            ("down", 125),
            ("left", 123),
            ("right", 124),
            ("a", 0),
            ("b", 11),
            ("c", 8),
            ("d", 2),
            ("e", 14),
            ("f", 3),
            ("g", 5),
            ("h", 4),
            ("i", 34),
            ("j", 38),
            ("k", 40),
            ("l", 37),
            ("m", 46),
            ("n", 45),
            ("o", 31),
            ("p", 35),
            ("q", 12),
            ("r", 15),
            ("s", 1),
            ("t", 17),
            ("u", 32),
            ("v", 9),
            ("w", 13),
            ("x", 7),
            ("y", 16),
            ("z", 6),
        ];

        for (name, keycode) in expected {
            assert_eq!(keycodes.get(name), Some(&keycode), "{name}");
        }
    }

    #[test]
    fn aliases_share_virtual_keycodes() {
        let keycodes = name_to_keycode();
        for (alias, canonical) in [
            ("enter", "return"),
            ("esc", "escape"),
            ("uparrow", "up"),
            ("downarrow", "down"),
            ("leftarrow", "left"),
            ("rightarrow", "right"),
        ] {
            assert_eq!(keycodes.get(alias), keycodes.get(canonical), "{alias}");
        }
    }
}
