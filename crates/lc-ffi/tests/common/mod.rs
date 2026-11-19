//! The demo network driven as the apps drive it (docs/testing.md): companion frames both ways,
//! the clock, `tick`, events, on simulated time, so a scenario that waits for the demo's 200 s
//! event takes milliseconds.
//!
//! Each `scenarios_*.rs` file includes this module; not every file uses every helper.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use lc_ffi::{
    ChatRef, ContactInfo, Conversation, DemoNetwork, GroupInfo, LichenCore, LichenEvent, Message,
    MessageStatus, StoredMessage,
};

/// One pump of the app's loop, in simulated milliseconds (what the existing demo tests use).
pub const STEP_MS: u64 = 100;

/// The demo network and the app's core, pumped as the apps pump them.
pub struct Sim {
    pub demo: Arc<DemoNetwork>,
    pub core: Arc<LichenCore>,
    /// Simulated milliseconds since the demo started.
    pub now: u64,
    /// The wall clock at `now` 0, Unix milliseconds (the demo's bots run on the real clock).
    pub wall0: u64,
    /// Every event the app polled, with when.
    pub events: Vec<(u64, LichenEvent)>,
    /// The identity seed and store folder, to restart the app on them.
    pub seed: u8,
    pub dir: Option<TempDir>,
    /// The demo's radio switched off (Settings › Demo, `-offline`): the network goes on, what
    /// it pushes is lost, nothing is written to it.
    pub radio_off: bool,
    /// Introductions stay requests (`-requestDemo YES`).
    requests: bool,
    restarts: u8,
}

pub fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A folder removed when the test ends.
#[derive(Debug)]
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lichen-scenario-{}-{name}-{}",
            std::process::id(),
            unix_now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The app's core as the apps make it: an identity seed of `seed` bytes, the history in `dir`
/// (in memory without).
pub fn new_core(seed: u8, rng: u8, dir: Option<&TempDir>) -> Arc<LichenCore> {
    LichenCore::new(
        "lichen-test".into(),
        vec![rng; 32],
        vec![seed; 32],
        dir.map(TempDir::path),
    )
    .unwrap()
}

impl Sim {
    /// The demo network and an in-memory core, connected at 0, set up as the apps set up the
    /// demo: its friends' groups joined without asking (`set_group_consent(false)`), their
    /// introductions not requests (`set_request_consent(false)`).
    pub fn new(seed: u8) -> Self {
        Self::build(seed, None, DemoNetwork::new(), false)
    }

    /// As `new`, the history kept in a folder, to restart on it.
    pub fn persistent(seed: u8, name: &str) -> Self {
        Self::build(seed, Some(TempDir::new(name)), DemoNetwork::new(), false)
    }

    /// As `new`, on a demo network set up first (`marc_as_repeater`, `julien_sos`, …);
    /// `requests`: introductions stay requests (`-requestDemo YES`, ADR-0046).
    pub fn on(seed: u8, demo: Arc<DemoNetwork>, requests: bool) -> Self {
        Self::build(seed, None, demo, requests)
    }

    fn build(seed: u8, dir: Option<TempDir>, demo: Arc<DemoNetwork>, requests: bool) -> Self {
        let core = new_core(seed, seed.wrapping_add(100), dir.as_ref());
        core.set_group_consent(false);
        if !requests {
            core.set_request_consent(false);
        }
        let wall0 = unix_now_ms();
        core.set_clock(wall0, 0);
        core.connect(0);
        Self {
            demo,
            core,
            now: 0,
            wall0,
            events: Vec::new(),
            seed,
            dir,
            radio_off: false,
            requests,
            restarts: 0,
        }
    }

    /// Unix seconds now, as the apps date what they send.
    pub fn unix_s(&self) -> u32 {
        ((self.wall0 + self.now) / 1000) as u32
    }

    /// Unix milliseconds now.
    pub fn unix_ms(&self) -> i64 {
        (self.wall0 + self.now) as i64
    }

    /// One pump, as the apps' demo runs it: the companion's frames in, the clock, `tick`, the
    /// battery when due, frames out, events. Returns the events of this pump.
    pub fn step(&mut self) -> Vec<LichenEvent> {
        let now = self.now;
        for f in self.demo.read(now) {
            if !self.radio_off {
                self.core.on_companion_frame(f, now);
            }
        }
        self.core.set_clock(self.wall0 + now, now);
        self.core.tick(now);
        self.core.refresh_battery_if_due(now);
        while let Some(f) = self.core.poll_companion(now) {
            if !self.radio_off {
                self.demo.write(f, now);
            }
        }
        let mut got = Vec::new();
        while let Some(e) = self.core.poll_event() {
            self.events.push((now, e.clone()));
            got.push(e);
        }
        self.now += STEP_MS;
        got
    }

    /// Events the app polls outside a pump (after an API call), recorded like the others.
    pub fn drain(&mut self) -> Vec<LichenEvent> {
        let mut got = Vec::new();
        while let Some(e) = self.core.poll_event() {
            self.events.push((self.now, e.clone()));
            got.push(e);
        }
        got
    }

    /// Runs for `ms` simulated milliseconds.
    pub fn run_for(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.step();
        }
    }

    /// Runs to demo time `at_ms` (nothing when already past it).
    pub fn run_to(&mut self, at_ms: u64) {
        while self.now < at_ms {
            self.step();
        }
    }

    /// Runs until `found` returns something for an event, or fails after `limit_s` simulated
    /// seconds, naming `what` and the events seen meanwhile.
    pub fn until_event<T>(
        &mut self,
        what: &str,
        limit_s: u64,
        mut found: impl FnMut(&LichenEvent) -> Option<T>,
    ) -> T {
        let end = self.now + limit_s * 1000;
        let from = self.events.len();
        while self.now < end {
            for e in self.step() {
                if let Some(t) = found(&e) {
                    return t;
                }
            }
        }
        panic!(
            "{what}: not within {limit_s} s (at {} s); events meanwhile:\n{}",
            self.now / 1000,
            summary(&self.events[from..])
        );
    }

    /// Runs until `done` holds for the core, or fails after `limit_s` simulated seconds.
    pub fn until(&mut self, what: &str, limit_s: u64, mut done: impl FnMut(&LichenCore) -> bool) {
        let end = self.now + limit_s * 1000;
        let from = self.events.len();
        while self.now < end {
            if done(&self.core) {
                return;
            }
            self.step();
        }
        if done(&self.core) {
            return;
        }
        panic!(
            "{what}: not within {limit_s} s (at {} s); events meanwhile:\n{}",
            self.now / 1000,
            summary(&self.events[from..])
        );
    }

    /// Runs until `pick` finds something in the core's state.
    pub fn until_some<T>(
        &mut self,
        what: &str,
        limit_s: u64,
        mut pick: impl FnMut(&LichenCore) -> Option<T>,
    ) -> T {
        let mut out = None;
        self.until(what, limit_s, |c| {
            out = pick(c);
            out.is_some()
        });
        out.unwrap()
    }

    /// The contact named `name` (its nickname or its node's name), once the radio listed it.
    pub fn contact(&mut self, name: &str) -> ContactInfo {
        self.until_some(&format!("contact {name}"), 120, |c| find_contact(c, name))
    }

    /// Camille, once her identity is known: someone to write to (she says hello at 30 s).
    pub fn camille(&mut self) -> ContactInfo {
        self.until_some("Camille's identity", 120, |c| {
            find_contact(c, "Camille").filter(|k| k.identity.is_some())
        })
    }

    /// The demo group "Rando Vercors", once its keys are all known (Camille creates it at 60 s,
    /// after a first exchange with the app).
    pub fn rando(&mut self) -> GroupInfo {
        self.until_some("the group Rando Vercors", 600, |c| {
            c.groups()
                .into_iter()
                .find(|g| g.name == "Rando Vercors" && g.keys_known + 1 == g.members.len() as u32)
        })
    }

    /// Switches the demo's radio off or on, as Settings › Demo and `-offline` / `-online` do.
    pub fn set_radio(&mut self, on: bool) {
        self.radio_off = !on;
        if on {
            self.core.connect(self.now);
        } else {
            self.core.disconnect(self.now);
        }
    }

    /// The app is killed and starts again: the core is dropped as it is, a new one opens the
    /// same folder with the same identity seed, and connects to the same radio `after_ms`
    /// later. The demo's friends are set up again as the apps do at each launch.
    pub fn restart(&mut self, after_ms: u64) {
        let dir = self.dir.as_ref().expect("a persistent sim");
        self.restarts += 1;
        let core = new_core(self.seed, self.seed.wrapping_add(self.restarts), Some(dir));
        self.core = core;
        self.core.set_group_consent(false);
        if !self.requests {
            self.core.set_request_consent(false);
        }
        self.core.set_clock(self.wall0 + self.now, self.now);
        self.now += after_ms;
        // The app connects to its radio at launch.
        self.radio_off = false;
        self.core.connect(self.now);
    }

    /// The history of `chat`, oldest first (up to 500 messages).
    pub fn history(&self, chat: &ChatRef) -> Vec<StoredMessage> {
        self.core.history(chat.clone(), None, 500).unwrap()
    }

    /// The conversation of `chat` in Messages.
    pub fn conversation(&self, chat: &ChatRef) -> Option<Conversation> {
        self.core
            .conversations()
            .into_iter()
            .find(|c| &c.chat == chat)
    }

    /// The message `id` of `chat`, as the conversation shows it now.
    pub fn message(&self, chat: &ChatRef, id: i64) -> StoredMessage {
        self.history(chat)
            .into_iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("message {id} not in {chat:?}"))
    }

    /// Runs until message `id` of `chat` has the status `want` says.
    pub fn until_status(
        &mut self,
        chat: &ChatRef,
        id: i64,
        limit_s: u64,
        want: impl Fn(&MessageStatus) -> bool,
    ) -> StoredMessage {
        let chat = chat.clone();
        self.until_some(&format!("message {id}'s status"), limit_s, |c| {
            c.history(chat.clone(), None, 500)
                .unwrap()
                .into_iter()
                .find(|m| m.id == id && want(&m.status))
        })
    }

    /// The events seen so far, after index `from`.
    pub fn since(&self, from: usize) -> impl Iterator<Item = &LichenEvent> {
        self.events[from..].iter().map(|(_, e)| e)
    }
}

pub fn find_contact(c: &LichenCore, name: &str) -> Option<ContactInfo> {
    c.contacts()
        .into_iter()
        .find(|k| k.name == name || k.node_name == name)
}

pub fn contact_chat(c: &ContactInfo) -> ChatRef {
    ChatRef::Contact { key: c.key.clone() }
}

/// A message's text, if it is one.
pub fn text(m: &Message) -> Option<&str> {
    match m {
        Message::Text { text, .. } => Some(text),
        _ => None,
    }
}

/// The texts of a history, in order.
pub fn texts(h: &[StoredMessage]) -> Vec<String> {
    h.iter()
        .filter_map(|m| text(&m.message).map(str::to_owned))
        .collect()
}

/// How many messages of `h` say exactly `s`.
pub fn copies(h: &[StoredMessage], s: &str) -> usize {
    h.iter().filter(|m| text(&m.message) == Some(s)).count()
}

fn summary(events: &[(u64, LichenEvent)]) -> String {
    let mut out = String::new();
    // The last 40 are enough to see what went on.
    let skip = events.len().saturating_sub(40);
    for (at, e) in &events[skip..] {
        let line = match e {
            LichenEvent::MessageChanged { message } => format!(
                "MessageChanged #{} {:?} out={} {:?}",
                message.id,
                message.chat,
                message.outgoing,
                short(&message.message)
            ),
            LichenEvent::Received { message, .. } => format!("Received {:?}", short(message)),
            LichenEvent::GroupMessage { message, late, .. } => {
                format!("GroupMessage {:?} late={late}", short(message))
            }
            other => {
                let s = format!("{other:?}");
                s.chars().take(120).collect()
            }
        };
        out.push_str(&format!("  {:>6.1} s  {line}\n", *at as f64 / 1000.0));
    }
    out
}

fn short(m: &Message) -> String {
    match m {
        Message::Text { text, .. } => format!("text {text:?}"),
        Message::Image { full, .. } => format!("image full={full}"),
        Message::Voice { frames, .. } => format!("voice {frames}"),
        Message::Notice { kind, text, .. } => format!("notice {kind} {text:?}"),
        other => format!("{other:?}").chars().take(60).collect(),
    }
}
