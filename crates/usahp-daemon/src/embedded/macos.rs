use super::Driver;
use anyhow::{Result, bail};
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_graphics::event::{
    CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
    CallbackResult,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
}
const CODES: &[(&str, u16)] = &[
    ("Space", 49),
    ("Enter", 36),
    ("Backspace", 51),
    ("Tab", 48),
    ("Escape", 53),
    ("ArrowUp", 126),
    ("ArrowDown", 125),
    ("ArrowLeft", 123),
    ("ArrowRight", 124),
    ("Home", 115),
    ("End", 119),
    ("PageUp", 116),
    ("PageDown", 121),
    ("Insert", 114),
    ("Delete", 117),
    ("A", 0),
    ("B", 11),
    ("C", 8),
    ("D", 2),
    ("E", 14),
    ("F", 3),
    ("G", 5),
    ("H", 4),
    ("I", 34),
    ("J", 38),
    ("K", 40),
    ("L", 37),
    ("M", 46),
    ("N", 45),
    ("O", 31),
    ("P", 35),
    ("Q", 12),
    ("R", 15),
    ("S", 1),
    ("T", 17),
    ("U", 32),
    ("V", 9),
    ("W", 13),
    ("X", 7),
    ("Y", 16),
    ("Z", 6),
    ("0", 29),
    ("1", 18),
    ("2", 19),
    ("3", 20),
    ("4", 21),
    ("5", 23),
    ("6", 22),
    ("7", 26),
    ("8", 28),
    ("9", 25),
    ("F1", 122),
    ("F2", 120),
    ("F3", 99),
    ("F4", 118),
    ("F5", 96),
    ("F6", 97),
    ("F7", 98),
    ("F8", 100),
    ("F9", 101),
    ("F10", 109),
    ("F11", 103),
    ("F12", 111),
    ("F13", 105),
    ("F14", 107),
    ("F15", 113),
    ("F16", 106),
    ("F17", 64),
    ("F18", 79),
    ("F19", 80),
    ("F20", 90),
];
pub fn code_for_name(name: &str) -> Option<u16> {
    CODES.iter().find_map(|(n, c)| (*n == name).then_some(*c))
}
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Capture {
    pub fn start(driver: Driver) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("usahp-embedded-tap".into())
            .spawn(move || {
                let events = driver.clone();
                let installed = CGEventTap::with_enabled(
                    CGEventTapLocation::Session,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![CGEventType::KeyDown, CGEventType::KeyUp],
                    move |_, kind, event| {
                        if !matches!(kind, CGEventType::KeyDown | CGEventType::KeyUp) {
                            events.lost();
                            return CallbackResult::Keep;
                        }
                        let code = event.get_integer_value_field(9) as u16;
                        if let Some((name, _)) = CODES.iter().find(|(_, c)| *c == code) {
                            if events.key(name, kind == CGEventType::KeyDown) {
                                return CallbackResult::Drop;
                            }
                        }
                        CallbackResult::Keep
                    },
                    || {
                        for (name, code) in CODES {
                            if unsafe { CGEventSourceKeyState(1, *code) } { driver.key(name, true); }
                        }
                        driver.ready();
                        if tx.send(Ok(())).is_err() {
                            return;
                        }
                        while !stopping.load(Ordering::Acquire) {
                            unsafe {
                                CFRunLoop::run_in_mode(
                                    kCFRunLoopDefaultMode,
                                    Duration::from_millis(20),
                                    true,
                                );
                            }
                            driver.tick();
                        }
                    },
                );
                if installed.is_err() {
                    let _ = tx.send(Err(
                        "Grant Accessibility permission to the host application before enabling switches."
                            .to_string(),
                    ));
                }
            })?;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(message)) => {
                let _ = thread.join();
                bail!(message);
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                bail!("macOS switch capture did not acknowledge startup.");
            }
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
