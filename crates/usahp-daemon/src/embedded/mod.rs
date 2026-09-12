//! In-process managed capture. No socket, simulator, or synthetic release activation.
//! Native callbacks do bounded state work only; consumers drain typed events.
mod keys;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
use anyhow::{Result, bail};
pub use keys::normalize as normalize_key;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::Instant,
};
use usahp_core::{Action, InputKind, Mapping, SwitchStateMachine};
pub const HEARTBEAT_INTERVAL_MS: u64 = 500;
pub const HEARTBEAT_TIMEOUT_MS: u64 = 1500;
const QUEUE_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Disabled,
    Escape,
    HoldEscape,
    HeartbeatTimeout,
    QueueOverflow,
    CaptureLost,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Switch {
        generation: u64,
        switch_id: String,
        action: Action,
        monotonic_ms: u64,
    },
    Learned {
        generation: u64,
        code: String,
    },
    Stopped {
        generation: u64,
        reason: StopReason,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Learning,
    Active,
}
#[derive(Debug, Clone, Copy)]
pub struct Status {
    pub generation: u64,
    pub mode: Mode,
    pub reason: Option<StopReason>,
}
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
struct Core {
    status: Status,
    native_lost: bool,
    mappings: HashMap<String, String>,
    logical: SwitchStateMachine,
    physical: HashSet<String>,
    down: HashSet<String>,
    held: HashMap<String, u64>,
    learned: Option<String>,
    escape_ms: u64,
    last_heartbeat: u64,
    events: VecDeque<Event>,
}
impl Default for Core {
    fn default() -> Self {
        Self {
            status: Status {
                generation: 0,
                mode: Mode::Off,
                reason: None,
            },
            native_lost: false,
            mappings: HashMap::new(),
            logical: SwitchStateMachine::new(&[]),
            physical: HashSet::new(),
            down: HashSet::new(),
            held: HashMap::new(),
            learned: None,
            escape_ms: 4000,
            last_heartbeat: 0,
            events: VecDeque::new(),
        }
    }
}
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
impl Core {
    fn stop(&mut self, reason: StopReason) {
        self.status.mode = Mode::Off;
        self.status.reason = Some(reason);
        self.logical.release_all();
        self.held.clear();
        self.learned = None;
        // Discard pending edges atomically. Reset releases are never physical edges.
        self.events.clear();
        self.events.push_back(Event::Stopped {
            generation: self.status.generation,
            reason,
        });
    }
    fn begin(&mut self, mode: Mode, now: u64) {
        self.stop(StopReason::Disabled);
        self.events.clear();
        self.status.generation = self.status.generation.wrapping_add(1);
        self.status.mode = mode;
        self.status.reason = None;
        self.last_heartbeat = now;
    }
    fn emit(&mut self, event: Event) {
        if self.events.len() >= QUEUE_LIMIT {
            self.stop(StopReason::QueueOverflow);
        } else {
            self.events.push_back(event);
        }
    }
    fn tick(&mut self, now: u64) {
        if self.status.mode == Mode::Off {
            return;
        }
        if now.saturating_sub(self.last_heartbeat) >= HEARTBEAT_TIMEOUT_MS {
            self.stop(StopReason::HeartbeatTimeout);
            return;
        }
        if self.status.mode == Mode::Active
            && self
                .held
                .values()
                .any(|start| now.saturating_sub(*start) >= self.escape_ms)
        {
            self.stop(StopReason::HoldEscape);
        }
    }
    fn key(&mut self, code: &str, pressed: bool, now: u64) -> bool {
        self.tick(now);
        let was_down = if pressed {
            !self.down.insert(code.into())
        } else {
            self.down.remove(code)
        };
        // Drain releases/repeats of consumed keys, even after cancellation.
        if self.status.mode == Mode::Off {
            return if pressed {
                self.physical.contains(code)
            } else {
                self.physical.remove(code)
            };
        }
        if code == "Escape" {
            if pressed {
                self.physical.insert(code.into());
                self.stop(StopReason::Escape);
            } else {
                self.physical.remove(code);
            }
            return true;
        }
        if self.status.mode == Mode::Learning {
            if pressed {
                if was_down {
                    return self.physical.contains(code);
                }
                self.physical.insert(code.into());
                if self.learned.is_none() {
                    self.learned = Some(code.into());
                }
            } else {
                if !self.physical.remove(code) {
                    return false;
                }
                if self.learned.as_deref() == Some(code) {
                    self.status.mode = Mode::Off;
                    self.learned = None;
                    self.emit(Event::Learned {
                        generation: self.status.generation,
                        code: code.into(),
                    });
                }
            }
            return true;
        }
        let Some(mapping_id) = self.mappings.get(code).cloned() else {
            return false;
        };
        if pressed {
            if was_down {
                return self.physical.contains(code);
            }
            if !self.physical.insert(code.into()) {
                return true;
            }
        } else if !self.physical.remove(code) {
            return false;
        }
        let action = if pressed {
            Action::Pressed
        } else {
            Action::Released
        };
        if let Ok(Some(edge)) =
            self.logical
                .apply(&mapping_id, action, Some(if pressed { 100.0 } else { 0.0 }))
        {
            if pressed {
                self.held.insert(edge.switch_id.clone(), now);
            } else {
                self.held.remove(&edge.switch_id);
            }
            self.emit(Event::Switch {
                generation: self.status.generation,
                switch_id: edge.switch_id,
                action,
                monotonic_ms: now,
            });
        }
        true
    }
}
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
#[derive(Clone)]
struct Driver {
    core: Arc<Mutex<Core>>,
    started: Instant,
}
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
impl Driver {
    fn now(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn key(&self, code: &str, pressed: bool) -> bool {
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .key(code, pressed, self.now())
    }
    fn tick(&self) {
        self.core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tick(self.now());
    }
    fn lost(&self) {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        core.native_lost = true;
        core.stop(StopReason::CaptureLost);
    }
    fn ready(&self, down: HashSet<String>) {
        let mut core = self.core.lock().unwrap_or_else(|p| p.into_inner());
        core.physical.retain(|key| down.contains(key));
        core.down = down;
        core.native_lost = false;
    }
}
/// One owner per process. Creation does not install hooks or capture input.
/// Call heartbeat at least every 500ms, independently of rendering.
/// Configure/learn only while off. Always stop and drain old gestures before re-enabling.
pub struct EmbeddedBroker {
    driver: Driver,
    #[cfg(target_os = "windows")]
    native: Option<windows::Capture>,
    #[cfg(target_os = "macos")]
    native: Option<macos::Capture>,
}
impl Default for EmbeddedBroker {
    fn default() -> Self {
        Self::new()
    }
}
impl EmbeddedBroker {
    pub fn new() -> Self {
        Self {
            driver: Driver {
                core: Arc::new(Mutex::new(Core::default())),
                started: Instant::now(),
            },
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            native: None,
        }
    }
    pub fn status(&self) -> Status {
        self.driver
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .status
    }
    pub fn heartbeat(&self) {
        let mut core = self.driver.core.lock().unwrap_or_else(|p| p.into_inner());
        core.last_heartbeat = self.driver.now();
    }
    pub fn drain(&self) -> Vec<Event> {
        let mut core = self.driver.core.lock().unwrap_or_else(|p| p.into_inner());
        core.tick(self.driver.now());
        core.events.drain(..).collect()
    }
    pub fn configure(&mut self, mappings: &[Mapping], escape_ms: u64) -> Result<()> {
        if self.status().mode != Mode::Off {
            bail!("Disable capture before changing mappings.");
        }
        if mappings.len() > 128 || escape_ms < 4000 {
            bail!("Invalid embedded mapping count or escape duration.");
        }
        let mut keys = HashMap::new();
        let mut ids = HashSet::new();
        for m in mappings {
            if m.input != InputKind::Keyboard
                || m.device.is_some()
                || m.switch_id.trim().is_empty()
                || !ids.insert(m.id.clone())
            {
                bail!("Invalid embedded keyboard mapping.");
            }
            let Some(code) = normalize_key(&m.code) else {
                bail!("Unsupported switch key: {}", m.code);
            };
            if code == "Escape" || !supported_key(&code) {
                bail!("Switch key is reserved or unavailable on this platform: {code}");
            }
            if keys.insert(code, m.id.clone()).is_some() {
                bail!("Each physical key can be mapped only once.");
            }
        }
        let mut core = self.driver.core.lock().unwrap_or_else(|p| p.into_inner());
        core.mappings = keys;
        core.logical = SwitchStateMachine::new(mappings);
        core.escape_ms = escape_ms;
        Ok(())
    }
    fn ensure_native(&mut self) -> Result<()> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if self
            .driver
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .native_lost
        {
            self.native.take();
        }
        #[cfg(target_os = "windows")]
        {
            if self.native.is_none() {
                self.native = Some(windows::Capture::start(self.driver.clone())?);
            }
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            if self.native.is_none() {
                self.native = Some(macos::Capture::start(self.driver.clone())?);
            }
            Ok(())
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            bail!("Embedded keyboard capture is supported on Windows and macOS.");
        }
    }
    fn begin(&mut self, mode: Mode) -> Result<u64> {
        if self.status().mode != Mode::Off {
            bail!("Capture is already active.");
        }
        self.ensure_native()?;
        let mut core = self.driver.core.lock().unwrap_or_else(|p| p.into_inner());
        if core.native_lost {
            bail!("Native switch capture was lost during startup.");
        }
        if !core.physical.is_empty() || !core.down.is_empty() {
            bail!("Release the held switch before starting capture.");
        }
        core.begin(mode, self.driver.now());
        Ok(core.status.generation)
    }
    pub fn enable(&mut self) -> Result<u64> {
        self.begin(Mode::Active)
    }
    pub fn learn(&mut self) -> Result<u64> {
        self.begin(Mode::Learning)
    }
    pub fn monotonic_ms(&self) -> u64 {
        self.driver.now()
    }
    pub fn shutdown(&mut self) {
        self.stop();
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            self.native.take();
        }
        let mut core = self.driver.core.lock().unwrap_or_else(|p| p.into_inner());
        core.physical.clear();
        core.down.clear();
    }
    pub fn stop(&self) {
        self.driver
            .core
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stop(StopReason::Disabled);
    }
}
impl Drop for EmbeddedBroker {
    fn drop(&mut self) {
        self.stop();
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            self.native.take();
        }
    }
}
pub fn supported_key(code: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::code_for_name(code).is_some()
    }
    #[cfg(target_os = "macos")]
    {
        macos::code_for_name(code).is_some()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = code;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn core() -> Core {
        let mut c = Core::default();
        let m = Mapping {
            id: "space".into(),
            switch_id: "select".into(),
            input: InputKind::Keyboard,
            code: "Space".into(),
            device: None,
        };
        c.mappings.insert("Space".into(), "space".into());
        c.logical = SwitchStateMachine::new(&[m]);
        c.begin(Mode::Active, 0);
        c
    }
    #[test]
    fn repeat_and_unmatched_release_never_activate() {
        let mut c = core();
        assert!(!c.key("Space", false, 0));
        assert!(c.events.is_empty());
        c.key("Space", true, 1);
        c.key("Space", true, 2);
        c.key("Space", false, 3);
        assert_eq!(c.events.len(), 2);
        assert!(!c.key("A", true, 4));
    }
    #[test]
    fn cancellation_discards_edges_without_synthetic_releases() {
        let mut c = core();
        c.key("Space", true, 1);
        c.stop(StopReason::Disabled);
        assert!(matches!(c.events.front(), Some(Event::Stopped { .. })));
        assert_eq!(c.events.len(), 1);
        assert!(c.key("Space", false, 2));
        assert_eq!(c.events.len(), 1);
    }
    #[test]
    fn heartbeat_and_long_hold_fail_open() {
        let mut c = core();
        c.key("Space", true, 0);
        c.tick(1500);
        assert_eq!(c.status.reason, Some(StopReason::HeartbeatTimeout));
        let mut c = core();
        c.escape_ms = 6000;
        c.key("Space", true, 0);
        for n in 1..=12 {
            c.last_heartbeat = n * 500;
            c.tick(n * 500);
        }
        assert_eq!(c.status.reason, Some(StopReason::HoldEscape));
        assert!(!c.key("A", true, 6001));
    }
    #[test]
    fn learns_only_complete_press_and_escape_cancels() {
        let mut c = Core::default();
        c.begin(Mode::Learning, 0);
        c.key("Space", false, 1);
        assert!(c.events.is_empty());
        c.key("Space", true, 2);
        c.key("Space", true, 3);
        c.key("Space", false, 4);
        assert!(matches!(c.events.front(),Some(Event::Learned{code,..}) if code=="Space"));
        c.begin(Mode::Learning, 5);
        c.key("Escape", true, 6);
        assert_eq!(c.status.reason, Some(StopReason::Escape));
    }
    #[test]
    fn overflow_cancels_instead_of_delivering_partial_gesture() {
        let mut c = core();
        for n in 0..300 {
            c.key("Space", n % 2 == 0, n);
        }
        assert_eq!(c.status.reason, Some(StopReason::QueueOverflow));
        assert_eq!(c.events.len(), 1);
        assert!(matches!(c.events[0], Event::Stopped { .. }));
    }
    #[test]
    fn learning_drains_overlapping_keys() {
        let mut c = Core::default();
        c.begin(Mode::Learning, 0);
        for (code, down) in [("A", true), ("B", true), ("B", false), ("A", false)] {
            assert!(c.key(code, down, 1));
        }
        assert!(c.physical.is_empty());
        assert!(c.down.is_empty());
        assert!(matches!(c.events.front(),Some(Event::Learned{code,..}) if code=="A"));
    }
    #[test]
    fn prior_passed_press_retains_passed_release() {
        let mut c = core();
        c.stop(StopReason::Disabled);
        assert!(!c.key("Space", true, 1));
        c.begin(Mode::Active, 2);
        assert!(!c.key("Space", true, 3));
        assert!(!c.key("Space", false, 4));
        assert!(c.events.is_empty());
    }
    #[test]
    fn cleanup_does_not_erase_native_failure() {
        let driver = Driver {
            core: Arc::new(Mutex::new(Core::default())),
            started: Instant::now(),
        };
        driver.core.lock().unwrap().begin(Mode::Learning, 0);
        driver.key("Space", true);
        driver.lost();
        driver.core.lock().unwrap().stop(StopReason::Disabled);
        assert!(driver.core.lock().unwrap().native_lost);
        driver.ready(HashSet::new());
        assert!(!driver.core.lock().unwrap().native_lost);
        assert!(driver.core.lock().unwrap().down.is_empty());
        assert!(driver.core.lock().unwrap().physical.is_empty());
    }
    #[test]
    fn generations_change_and_names_are_stable() {
        let mut c = core();
        let old = c.status.generation;
        c.stop(StopReason::Disabled);
        c.begin(Mode::Active, 1);
        assert_ne!(c.status.generation, old);
        assert_eq!(normalize_key("Return").as_deref(), Some("Enter"));
        assert!(normalize_key("F24").is_some());
        assert!(normalize_key("F25").is_none());
    }
}
