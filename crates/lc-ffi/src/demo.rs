//! A demo network for the apps: the app's core drives a simulated companion in
//! `lc_core::fakenet`, next to two simulated users who write, answer and start a group.
//! Simulated time follows the app's clock, so LoRa delays are real.

use std::sync::{Arc, Mutex};

use lc_core::content::MediaId;
use lc_core::fakenet::Net;
use lc_core::{Chat, Content, Event, Fix, GroupId, Precision};
use lc_meshcore::airtime::LoRaParams;

use crate::LocationFix;

/// The demo's places (Vercors): the refuge where Camille waits, the col Julien leaves from,
/// and where the app's user stands.
const REFUGE: (f64, f64) = (44.8960, 5.4880);
const COL: (f64, f64) = (44.9150, 5.4620);
const YOU: (f64, f64) = (44.8885, 5.4985);
/// Julien's walking speed, m/s.
const WALK: f64 = 1.3;
/// Camille's voice message: `mode u8 ‖ frames u16 ‖ Codec2 data` (macOS speech synthesis,
/// encoded by `cargo run -p lc-media --example encode_voice`).
const CAMILLE_VOICE: &[u8] = include_bytes!("../../../testdata/voice/camille-refuge.c2");

#[derive(Clone, Debug)]
enum Action {
    Dm {
        from: usize,
        text: String,
    },
    CreateGroup,
    /// Camille shares her exact position with the app's user for two hours.
    CamilleShares,
    /// Camille sends the preview of a photo of the refuge.
    CamillePhoto,
    /// Camille answers with a voice message.
    CamilleVoice,
    /// Camille asks for the full variant of the user's image.
    CamilleWantsFull(MediaId),
    GroupPost {
        from: usize,
        text: String,
    },
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
    /// When Julien started walking (simulated ms), once he shares with the group.
    julien_walk: Option<u64>,
    next_fix: u64,
    /// Wall clock at simulated time 0, Unix milliseconds.
    wall0: u64,
    /// The full variant of Camille's photo, kept for when it is asked for.
    camille_full: Option<(MediaId, Vec<u8>)>,
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A fix at `(lat, lon)`, moving at `speed` m/s on `heading`, measured at `unix_s`.
fn fix(at: (f64, f64), heading: f64, speed: f64, unix_s: u64) -> Fix {
    Fix {
        lat: at.0,
        lon: at.1,
        alt_m: Some(1_650.0),
        accuracy_m: 6.0,
        heading_deg: Some(heading),
        speed_mps: Some(speed),
        unix_s,
    }
}

/// Where Julien is `t_s` seconds after leaving the col, and his heading and speed.
fn julien_at(t_s: f64) -> ((f64, f64), f64, f64) {
    let m = 111_320.0;
    let dy = (REFUGE.0 - COL.0) * m;
    let dx = (REFUGE.1 - COL.1) * m * COL.0.to_radians().cos();
    let d = dx.hypot(dy);
    let f = (WALK * t_s / d).min(1.0);
    let heading = dx.atan2(dy).to_degrees().rem_euclid(360.0);
    let pos = (
        COL.0 + (REFUGE.0 - COL.0) * f,
        COL.1 + (REFUGE.1 - COL.1) * f,
    );
    (pos, heading, if f < 1.0 { WALK } else { 0.0 })
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
    /// The bots' wall clock, Unix seconds.
    const fn unix_s(&self) -> u64 {
        (self.wall0 + self.net.now) / 1000
    }

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
            Action::CamilleShares => {
                let t = self.unix_s();
                let until = t + 2 * 3600;
                let _ = self.net.with_core(self.camille, |c, now| {
                    c.update_location(fix(REFUGE, 0.0, 0.0, t), now);
                    c.start_share(Chat::Contact(app_key), Precision::Exact, until, now)
                });
            }
            Action::CamillePhoto => {
                let (w, h) = (640, 480);
                let Ok(t) =
                    lc_media::image::thumbnail(&lc_media::image::sample_landscape(w, h), w, h)
                else {
                    return;
                };
                let media_id = [0xCA, 0x31, 0x11, 0xE0, 0, 0, 0, 1];
                let variant = |full: bool, data: Vec<u8>| Content::Image {
                    sent_at: ts,
                    media_id,
                    full,
                    width: t.width,
                    height: t.height,
                    full_len: t.full.len() as u16,
                    data,
                };
                self.camille_full = Some((media_id, variant(true, t.full.clone()).encode()));
                let preview = variant(false, t.preview.clone()).encode();
                let _ = self.net.with_core(self.camille, |c, now| {
                    c.send_text(&app_key, "The view from the refuge tonight:", ts, now)?;
                    c.send_media(&app_key, preview, now)
                });
            }
            Action::CamilleVoice => {
                if let Some((head, data)) = CAMILLE_VOICE.split_first_chunk::<3>() {
                    let voice = Content::Voice {
                        sent_at: ts,
                        mode: head[0],
                        frames: u16::from_le_bytes([head[1], head[2]]),
                        data: data.to_vec(),
                    };
                    let _ = self.net.with_core(self.camille, |c, now| {
                        c.send_media(&app_key, voice.encode(), now)
                    });
                }
            }
            Action::CamilleWantsFull(media_id) => {
                let request = Content::MediaRequest {
                    media_id,
                    full: true,
                }
                .encode();
                let _ = self.net.with_core(self.camille, |c, now| {
                    c.send_datagram(&app_key, request, now)
                });
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
        self.locate();
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
                    Event::Received {
                        from,
                        content: Content::Voice { .. },
                        ..
                    } if from == app_key && bot == self.camille => {
                        self.pending.push((now + 6 * SEC, Action::CamilleVoice));
                    }
                    Event::Received {
                        from,
                        content: Content::Image { full, media_id, .. },
                        ..
                    } if from == app_key && bot == self.camille => {
                        let next = if full {
                            Action::Dm {
                                from: bot,
                                text: "Got the full picture, thanks!".into(),
                            }
                        } else {
                            Action::CamilleWantsFull(media_id)
                        };
                        self.pending.push((now + 4 * SEC, next));
                    }
                    Event::MediaRequested { from, media_id, .. }
                        if from == app_key && bot == self.camille =>
                    {
                        if let Some((id, datagram)) = &self.camille_full
                            && *id == media_id
                        {
                            let datagram = datagram.clone();
                            let _ = self
                                .net
                                .with_core(bot, |c, t| c.send_media(&app_key, datagram, t));
                        }
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
            // He shares his position with the group, rounded to 1 km, as he leaves the col.
            self.julien_walk = Some(now);
            let unix = self.unix_s();
            let until = unix + 4 * 3600;
            let _ = self.net.with_core(self.julien, |c, t| {
                c.update_location(fix(COL, 0.0, 0.0, unix), t);
                c.start_share(Chat::Group(gid), Precision::Km1, until, t)
            });
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

impl Demo {
    /// The bots' phones report their positions every 10 seconds.
    fn locate(&mut self) {
        let now = self.net.now;
        if now < self.next_fix {
            return;
        }
        self.next_fix = now + 10 * SEC;
        let unix = self.unix_s();
        let _ = self.net.with_core(self.camille, |c, t| {
            c.update_location(fix(REFUGE, 0.0, 0.0, unix), t);
        });
        if let Some(start) = self.julien_walk {
            let (at, heading, speed) = julien_at((now - start) as f64 / 1000.0);
            let _ = self.net.with_core(self.julien, |c, t| {
                c.update_location(fix(at, heading, speed, unix), t);
            });
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
        // The bots' clocks are the real one (the demo runs in real time).
        let wall = unix_now_ms();
        for bot in [camille, julien] {
            let _ = net.with_core(bot, |c, now| c.set_clock(wall, now));
        }
        let pending = vec![
            (
                30 * SEC,
                Action::Dm {
                    from: camille,
                    text: "Hi! I'm at the refuge, the mobile network doesn't reach up here.".into(),
                },
            ),
            (60 * SEC, Action::CreateGroup),
            (90 * SEC, Action::CamilleShares),
            (150 * SEC, Action::CamillePhoto),
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
                julien_walk: None,
                next_fix: 0,
                wall0: wall,
                camille_full: None,
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

    /// Where the app's user stands in the demo (instead of the phone's location).
    pub fn my_position(&self) -> LocationFix {
        LocationFix {
            lat: YOU.0,
            lon: YOU.1,
            alt_m: Some(1_480.0),
            accuracy_m: 8.0,
            heading_deg: None,
            speed_mps: None,
            unix_s: unix_now_ms() / 1000,
        }
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
        let core = LichenCore::new("demo-test".into(), vec![7; 32], None, None).unwrap();
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

#[cfg(test)]
mod location {
    use super::*;
    use crate::{LichenCore, LichenEvent, SharePrecision};

    /// Camille's exact position arrives directly, Julien's through the group rounded to 1 km;
    /// the map gets one marker each.
    #[test]
    fn bots_share_their_positions() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![4; 32], None, None).unwrap();
        core.set_clock(unix_now_ms(), 0);
        core.connect(0);
        let mut events = Vec::new();
        let mut shared = false;
        let mut now = 0;
        while now < 12 * 60_000 {
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
            if now % 10_000 == 0 {
                let mut me = demo.my_position();
                me.unix_s = (unix_now_ms() + now) / 1000;
                core.update_location(me, now);
            }
            if !shared && let Some(c) = core.contacts().into_iter().find(|c| c.name == "Camille") {
                core.start_share(
                    crate::ChatRef::Contact { key: c.key },
                    SharePrecision::M100,
                    u64::MAX,
                    now,
                )
                .unwrap();
                shared = true;
            }
            now += 100;
        }
        assert_eq!(core.shares().len(), 1);
        let got: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                LichenEvent::Location { position, .. } => Some(position.clone()),
                _ => None,
            })
            .collect();
        assert!(got.iter().any(|p| p.group.is_none() && p.radius_m < 10.0));
        assert!(got.iter().any(|p| p.group.is_some() && p.radius_m > 700.0));
        let map = core.positions(now);
        assert_eq!(map.len(), 2, "{map:?}");
        let camille = map.iter().find(|p| p.group.is_none()).unwrap();
        assert!(lc_core::location::distance_m((camille.lat, camille.lon), REFUGE) < 5.0);
        let julien = map.iter().find(|p| p.group.is_some()).unwrap();
        assert!(lc_core::location::distance_m((julien.lat, julien.lon), COL) < 2_500.0);
    }
}

#[cfg(test)]
mod media {
    use super::*;
    use crate::{LichenCore, LichenEvent, Message, StoredMessage};

    /// Voice both ways, Camille's photo loaded in full on request, ours sent in full when
    /// Camille asks.
    #[test]
    fn voice_and_images_with_camille() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![5; 32], None, None).unwrap();
        core.connect(0);
        let mut changed: Vec<StoredMessage> = Vec::new();
        let mut step = 0;
        let mut now = 0;
        while now < 9 * 60_000 {
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                if let LichenEvent::MessageChanged { message } = e {
                    changed.push(message);
                }
            }
            let camille = core.contacts().into_iter().find(|c| c.name == "Camille");
            if let Some(c) = camille {
                if step == 0 && now > 40_000 {
                    let pcm: Vec<i16> = (0..24_000).map(|i| ((i % 80) as i16 - 40) * 200).collect();
                    let v = crate::prepare_voice(pcm);
                    assert_eq!((v.mode, v.frames, v.data.len()), (1, 75, 450));
                    core.send_voice(c.key.clone(), v, 1, now).unwrap();
                    step = 1;
                }
                if step == 1
                    && let Some(p) = changed.iter().find(|m| {
                        matches!(m.message, Message::Image { full: false, .. }) && !m.outgoing
                    })
                {
                    core.request_full_image(p.id, now).unwrap();
                    step = 2;
                }
                if step == 2 && now > 6 * 60_000 {
                    let (w, h) = (320, 240);
                    let img = crate::prepare_image(lc_media::image::sample_landscape(w, h), w, h)
                        .unwrap();
                    core.send_image(c.key, img, 2, now).unwrap();
                    step = 3;
                }
            }
            now += 100;
        }
        assert_eq!(step, 3);
        // Camille's voice message, playable.
        let voice = changed
            .iter()
            .find_map(|m| match &m.message {
                Message::Voice {
                    mode, frames, data, ..
                } if !m.outgoing => Some((*mode, *frames, data.clone())),
                _ => None,
            })
            .expect("Camille's voice message");
        assert_eq!(voice.1, 154);
        assert_eq!(
            crate::decode_voice(voice.0, voice.1, voice.2)
                .unwrap()
                .len(),
            154 * 320
        );
        // Her photo: the preview, then the same message in full.
        let preview = changed
            .iter()
            .find(|m| matches!(m.message, Message::Image { full: false, .. }))
            .unwrap();
        let full = changed
            .iter()
            .find(|m| m.id == preview.id && matches!(m.message, Message::Image { full: true, .. }))
            .expect("the full photo replaced the preview");
        match (&preview.message, &full.message) {
            (
                Message::Image {
                    data: p, full_len, ..
                },
                Message::Image { data: f, .. },
            ) => {
                assert!(p.len() <= 450);
                assert_eq!(f.len(), usize::from(*full_len));
            }
            _ => unreachable!(),
        }
        // She asked for ours in full and said so.
        assert!(changed.iter().any(|m| matches!(
            &m.message,
            Message::Text { text, .. } if text == "Got the full picture, thanks!"
        )));
    }
}

#[cfg(test)]
mod persistence {
    use super::*;
    use crate::{ChatRef, LichenCore, LichenEvent, Message, MessageStatus};

    fn run(
        demo: &DemoNetwork,
        core: &LichenCore,
        from: u64,
        to: u64,
        mut each: impl FnMut(&LichenCore, u64),
    ) -> Vec<LichenEvent> {
        let mut events = Vec::new();
        let mut now = from;
        while now < to {
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
            each(core, now);
            now += 100;
        }
        events
    }

    fn text(m: &Message) -> &str {
        match m {
            Message::Text { text, .. } => text,
            Message::Voice { .. } | Message::Image { .. } | Message::Other { .. } => "",
        }
    }

    /// History, contacts and groups survive an app restart; the group keeps working both ways.
    #[test]
    fn restart_keeps_history_and_group() {
        let dir = std::env::temp_dir().join(format!("lichen-ffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = Some(dir.to_string_lossy().into_owned());
        let demo = DemoNetwork::new();

        let core = LichenCore::new("t".into(), vec![1; 32], None, data.clone()).unwrap();
        core.connect(0);
        let mut sent = false;
        run(&demo, &core, 0, 8 * 60_000, |c, now| {
            if !sent
                && now > 40_000
                && let Some(k) = c.contacts().into_iter().find(|k| k.name == "Camille")
            {
                sent = c.send_text(k.key, "first".into(), 1, now).is_ok();
            }
        });
        let camille = core
            .contacts()
            .into_iter()
            .find(|k| k.name == "Camille")
            .unwrap()
            .key;
        let before = core
            .history(
                ChatRef::Contact {
                    key: camille.clone(),
                },
                None,
                50,
            )
            .unwrap();
        assert!(before.len() >= 3, "{before:?}");
        assert!(
            before
                .iter()
                .any(|m| m.outgoing && m.status == MessageStatus::Delivered)
        );
        let group = core.groups().into_iter().next().expect("joined the group");
        let gref = ChatRef::Group {
            gid: group.gid.clone(),
        };
        assert!(!core.history(gref.clone(), None, 50).unwrap().is_empty());
        let prv = core.private_key().unwrap();
        drop(core);

        // The app restarts with the key from the Keychain: history before connecting.
        let core = LichenCore::new("t".into(), vec![2; 32], Some(prv), data.clone()).unwrap();
        assert_eq!(
            core.history(
                ChatRef::Contact {
                    key: camille.clone()
                },
                None,
                50
            )
            .unwrap(),
            before
        );
        assert_eq!(core.groups().len(), 1);
        assert!(!core.conversations().is_empty());

        core.connect(8 * 60_000);
        let mut step = 0;
        let events = run(&demo, &core, 8 * 60_000, 20 * 60_000, |c, now| {
            if step == 0 && now > 9 * 60_000 {
                step = 1;
                c.send_text(camille.clone(), "second".into(), 2, now)
                    .unwrap();
            }
            if step == 1 && now > 12 * 60_000 {
                step = 2;
                c.send_group_text(group.gid.clone(), "back online".into(), 3, now)
                    .unwrap();
            }
        });
        let changed: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                LichenEvent::MessageChanged { message } if !message.outgoing => {
                    Some(text(&message.message))
                }
                _ => None,
            })
            .collect();
        assert!(
            changed.iter().any(|t| t.starts_with("Copy: \"second\"")),
            "{changed:?}"
        );
        assert!(
            changed.contains(&"Seen. I'll keep a seat for you by the stove."),
            "{changed:?}"
        );
        let after = core
            .history(ChatRef::Contact { key: camille }, None, 50)
            .unwrap();
        assert!(after.len() >= before.len() + 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod unread {
    use super::*;
    use crate::LichenCore;

    #[test]
    fn unread_counts_received_messages() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![3; 32], None, None).unwrap();
        core.connect(0);
        let mut now = 0;
        while now < 60_000 {
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while core.poll_event().is_some() {}
            now += 100;
        }
        let conv = core.conversations();
        assert_eq!(conv.first().map(|c| c.unread), Some(1));
        core.mark_read(conv[0].chat.clone()).unwrap();
        assert_eq!(core.conversations()[0].unread, 0);
    }
}
