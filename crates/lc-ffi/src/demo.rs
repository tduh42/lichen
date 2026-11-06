//! A demo network for the apps: the app's core drives a simulated companion in
//! `lc_core::fakenet`, next to two simulated users who write, answer and start a group.
//! Simulated time follows the app's clock, so LoRa delays are real.

use std::sync::{Arc, Mutex};

use lc_core::fakenet::Net;
use lc_core::{Content, Event, GroupId};
use lc_meshcore::airtime::LoRaParams;

#[derive(Clone, Debug)]
enum Action {
    Dm { from: usize, text: String },
    CreateGroup,
    GroupPost { from: usize, text: String },
}

struct Demo {
    net: Net,
    app: usize,
    camille: usize,
    julien: usize,
    origin: Option<u64>,
    group: Option<GroupId>,
    pending: Vec<(u64, Action)>,
    julien_posted: bool,
}

/// A simulated MeshCore network for trying the app without a radio.
#[derive(uniffi::Object)]
pub struct DemoNetwork {
    inner: Mutex<Demo>,
}

impl std::fmt::Debug for DemoNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DemoNetwork")
    }
}

const SEC: u64 = 1_000;

impl Demo {
    fn sim_now(&mut self, app_now: u64) -> u64 {
        let origin = *self.origin.get_or_insert(app_now);
        app_now.saturating_sub(origin)
    }

    fn advance(&mut self, app_now: u64) {
        let t = self.sim_now(app_now);
        if t > self.net.now {
            self.net.run_until(t, |_| false);
        }
        self.bots();
    }

    fn act(&mut self, a: Action) {
        let app_key = self.net.key(self.app);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as u32);
        match a {
            Action::Dm { from, text } => {
                let _ = self
                    .net
                    .with_core(from, |c, now| c.send_text(&app_key, &text, ts, now));
            }
            Action::CreateGroup => {
                let members = [app_key, self.net.key(self.julien)];
                self.group = self
                    .net
                    .with_core(self.camille, |c, now| {
                        c.create_group("Rando Vercors", &members, now)
                    })
                    .and_then(Result::ok);
            }
            Action::GroupPost { from, text } => {
                if let Some(gid) = self.group {
                    let _ = self
                        .net
                        .with_core(from, |c, now| c.send_group_text(&gid, &text, ts, now));
                }
            }
        }
    }

    fn bots(&mut self) {
        let now = self.net.now;
        let due: Vec<Action> = self
            .pending
            .iter()
            .filter(|(at, _)| *at <= now)
            .map(|(_, a)| a.clone())
            .collect();
        self.pending.retain(|(at, _)| *at > now);
        for a in due {
            self.act(a);
        }
        let app_key = self.net.key(self.app);
        for (bot, name) in [(self.camille, "Camille"), (self.julien, "Julien")] {
            for e in self.net.take_events(bot) {
                match e {
                    Event::Received {
                        from,
                        content: Content::Text { text, .. },
                        ..
                    } if from == app_key => {
                        let reply = if name == "Camille" {
                            format!("Copy: \"{text}\". I'm at the refuge, one hop away.")
                        } else {
                            format!("Got it, two hops away: \"{text}\"")
                        };
                        self.pending.push((
                            now + 4 * SEC,
                            Action::Dm {
                                from: bot,
                                text: reply,
                            },
                        ));
                    }
                    Event::GroupMessage { from, .. } if from == app_key && bot == self.camille => {
                        self.pending.push((
                            now + 6 * SEC,
                            Action::GroupPost {
                                from: bot,
                                text: "Seen. I'll keep a seat for you by the stove.".into(),
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }
        // Julien posts once every member holds the keys.
        if !self.julien_posted
            && let Some(gid) = self.group
            && self
                .net
                .nodes
                .get(self.julien)
                .and_then(|n| n.core.as_ref())
                .is_some_and(|c| {
                    c.groups()
                        .any(|g| g.gid == gid && g.keys_known() + 1 == g.members.len())
                })
        {
            self.julien_posted = true;
            self.pending.push((
                now + 5 * SEC,
                Action::GroupPost {
                    from: self.julien,
                    text: "Leaving the col now, at the refuge around 2 pm.".into(),
                },
            ));
        }
    }
}

#[uniffi::export]
impl DemoNetwork {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        let radio = LoRaParams::meshcore(8, 62_500, 5).unwrap_or(LoRaParams {
            sf: 8,
            bw_hz: 62_500,
            cr: 5,
            preamble: 32,
        });
        let mut net = Net::new(radio, 5, 42);
        let app = net.add_external_companion(1, "ThinkNode M5");
        let hub = net.add_repeater(10, "hub");
        let crest = net.add_repeater(11, "crest");
        let camille = net.add_companion(2, "Camille");
        let julien = net.add_companion(3, "Julien");
        net.link(app, hub);
        net.link(hub, camille);
        net.link(hub, crest);
        net.link(crest, julien);
        for (a, b) in [(app, camille), (app, julien), (camille, julien)] {
            net.know(a, b, true);
            net.know(b, a, true);
        }
        net.connect(camille);
        net.connect(julien);
        let pending = vec![
            (
                30 * SEC,
                Action::Dm {
                    from: camille,
                    text: "Hi! I'm at the refuge, the mobile network doesn't reach up here.".into(),
                },
            ),
            (60 * SEC, Action::CreateGroup),
        ];
        Arc::new(Self {
            inner: Mutex::new(Demo {
                net,
                app,
                camille,
                julien,
                origin: None,
                group: None,
                pending,
                julien_posted: false,
            }),
        })
    }

    /// A frame from the app to its (simulated) companion.
    pub fn write(&self, frame: Vec<u8>, now_ms: u64) {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.advance(now_ms);
        let app = d.app;
        d.net.external_write(app, &frame);
    }

    /// Advances the simulation to `now_ms` and returns the companion's frames for the app.
    pub fn read(&self, now_ms: u64) -> Vec<Vec<u8>> {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.advance(now_ms);
        let app = d.app;
        d.net.external_read(app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LichenCore, LichenEvent, Message};

    /// The demo as the app runs it: frames back and forth, events out.
    #[test]
    fn demo_conversation_and_group() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("demo-test".into(), vec![7; 32], None).unwrap();
        core.connect(0);
        let mut events = Vec::new();
        let mut sent = false;
        for step in 0..(10 * 60 * 10) {
            let now = step * 100;
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                events.push(e);
            }
            if !sent
                && now > 40_000
                && let Some(c) = core.contacts().into_iter().find(|c| c.name == "Camille")
            {
                sent = core.send_text(c.key, "On my way".into(), 1, now).is_ok();
            }
        }
        let texts: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                LichenEvent::Received {
                    message: Message::Text { text, .. },
                    ..
                }
                | LichenEvent::GroupMessage {
                    message: Message::Text { text, .. },
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|t| t.starts_with("Hi! I'm at the refuge")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.starts_with("Copy: \"On my way\"")),
            "{texts:?}"
        );
        assert!(events.iter().any(
            |e| matches!(e, LichenEvent::GroupJoined { name, .. } if name == "Rando Vercors")
        ));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, LichenEvent::Delivered { .. }))
        );
        assert!(
            texts.iter().any(|t| t.starts_with("Leaving the col")),
            "{texts:?}"
        );
    }
}
