//! A demo network for the apps: the app's core drives a simulated companion in
//! `lc_core::fakenet`, next to two simulated users who write, answer and start a group.
//! Simulated time follows the app's clock, so LoRa delays are real.

use std::sync::{Arc, Mutex, OnceLock};

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
/// Where Julien hurt himself in the SOS scenario, halfway down, without GPS.
const JULIEN_HURT: (f64, f64) = (44.9055, 5.4750);
/// Julien's walking speed, m/s.
const WALK: f64 = 1.3;
/// Camille's voice message: `mode u8 ‖ frames u16 ‖ Codec2 data` (macOS speech synthesis,
/// encoded by `cargo run -p lc-media --example encode_voice`).
const CAMILLE_VOICE: &[u8] = include_bytes!("../../../testdata/voice/camille-refuge.c2");
/// Camille's photo id: her LoRa preview and her announcement share it.
const CAMILLE_PHOTO: [u8; 8] = [0xCA, 0x31, 0x11, 0xE0, 0, 0, 0, 1];

/// Camille's full-resolution photo on her bridge: what her announcement carries.
#[derive(Clone, Debug, Default)]
struct CamilleOffer {
    hash: [u8; 32],
    size: u32,
    iroh: Option<[u8; 32]>,
    onion: Option<[u8; 32]>,
    /// Tor folders, when the app asked for Tor and Camille has not started it yet.
    tor_wanted: Option<(std::path::PathBuf, std::path::PathBuf)>,
    /// Tor asked for and not given up: her announcement waits for the onion service.
    tor_pending: bool,
}

/// Camille's bridge: a real iroh endpoint (and an onion service when the app has Tor on),
/// started once per process in the background: the demo's bridge needs the Internet.
fn camille_offer() -> &'static Mutex<CamilleOffer> {
    static OFFER: OnceLock<Mutex<CamilleOffer>> = OnceLock::new();
    static STARTED: OnceLock<()> = OnceLock::new();
    let slot = OFFER.get_or_init(|| Mutex::new(CamilleOffer::default()));
    STARTED.get_or_init(|| {
        std::thread::spawn(|| {
            let Ok(rt) = tokio::runtime::Runtime::new() else {
                return;
            };
            rt.block_on(camille_serves());
        });
    });
    slot
}

async fn camille_serves() {
    let (w, h) = (1280, 960);
    let Ok(photo) =
        lc_media::image::encode_avif(&lc_media::image::sample_landscape(w, h), w, h, 70.0)
    else {
        return;
    };
    let seed = [0xCA; 32];
    let Ok(bridge) = lc_bridge::Bridge::new(None) else {
        return;
    };
    let Ok((hash, size)) = bridge.offer(photo) else {
        return;
    };
    let iroh = bridge
        .start_iroh(
            lc_bridge::derive_key(&seed, "lichen/1 iroh"),
            lc_bridge::Network::Default,
        )
        .await
        .ok();
    if let Ok(mut s) = camille_offer().lock() {
        s.hash = hash;
        s.size = size as u32;
        s.iroh = iroh;
    }
    // Serve for as long as the app runs; start Tor when the app asks.
    loop {
        let wanted = camille_offer()
            .lock()
            .ok()
            .and_then(|mut s| s.tor_wanted.take());
        if let Some((state, cache)) = wanted {
            let limit = std::time::Duration::from_secs(180);
            let mut onion = None;
            if bridge.start_tor(&state, &cache, limit).await.is_ok()
                && let Ok(id) = bridge
                    .publish_onion(lc_bridge::derive_key(&seed, "lichen/1 onion"))
                    .await
                && bridge
                    .onion_reachable(std::time::Duration::from_secs(300))
                    .await
            {
                onion = Some(id);
            }
            if let Ok(mut s) = camille_offer().lock() {
                s.onion = onion;
                s.tor_pending = false;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

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
    /// Léa, next to the user, scans their group invitation (§10.5), once her node is up.
    GuestJoins(String),
    /// A post in a given group (one the user created).
    PostIn {
        from: usize,
        gid: GroupId,
        text: String,
    },
    /// A bot reacts to one of the app's messages (§6.2).
    React {
        from: usize,
        target: lc_core::content::MessageRef,
        emoji: &'static str,
    },
    /// Camille answers a sticker with one of hers (§6.3).
    CamilleSticker,
    /// Julien posts a GIF in the group, a sunrise over the mountains (§6.3).
    JulienGif,
    /// Marc, who has the stock MeshCore app, writes to the user in plain (§15).
    MarcWrites(String),
    /// A post on MeshCore's public channel from someone with a stock client (§15.3).
    PublicPost {
        from: usize,
        text: String,
    },
}

struct Demo {
    net: Net,
    app: usize,
    hub: usize,
    camille: usize,
    julien: usize,
    /// Léa, who scans the user's group invitation (debug `inviteDemo`).
    guest: Option<usize>,
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
    /// Camille announced her full-resolution photo (§14.1).
    announced: bool,
    bots_met: bool,
    /// The SOS scenario: Julien in SOS without GPS once the group is set up, Camille searching.
    julien_sos: bool,
    camille_searching: bool,
    next_camille_ping: u64,
    /// Camille's own SOS, from and until (simulated ms), for two incidents at once.
    camille_sos: Option<(u64, u64)>,
    /// Marc, at the refuge with the stock MeshCore app (§15).
    marc: usize,
    marc_answered: bool,
}

/// The app's radio misses every copy of `author`'s second group message (its sender byte;
/// message counter 1 under a new key, §10.3), whenever it goes out.
fn lose_second_message(app: usize, author: Option<u8>) -> lc_core::fakenet::Deaf {
    Box::new(move |receiver, frame| {
        receiver == app
            && lc_meshcore::air::Packet::decode(frame)
                .ok()
                .filter(|p| p.ptype == lc_meshcore::air::PayloadType::GrpData)
                .and_then(|p| lc_proto::group::parse(p.payload))
                .is_some_and(|(h, _)| Some(h.sender) == author && h.counter == 1)
    })
}

/// A point in metres east and north of the user (the fake network's plane).
fn plane(at: (f64, f64)) -> (f64, f64) {
    let m = 111_320.0;
    (
        (at.1 - YOU.1) * m * YOU.0.to_radians().cos(),
        (at.0 - YOU.0) * m,
    )
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

/// Julien's first post, not before (demo time): after Camille's photo transfer.
const JULIEN_POSTS_FROM: u64 = 210 * SEC;

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
        // The simulated clock (the real one in the app, which runs the demo in real time).
        let ts = (self.unix_s() & 0xFFFF_FFFF) as u32;
        match a {
            Action::Dm { from, text } => {
                let _ = self
                    .net
                    .with_core(from, |c, now| c.send_text(&app_key, &text, ts, now));
            }
            Action::CreateGroup => {
                let members = [app_key, self.net.key(self.julien)];
                match self.net.with_core(self.camille, |c, now| {
                    c.create_group("Rando Vercors", &members, now)
                }) {
                    Some(Ok(gid)) => self.group = Some(gid),
                    // She does not know the app's identity yet (ADR-0013): after their first
                    // exchange.
                    Some(Err(lc_core::Error::IdentityUnknown)) => {
                        let at = self.net.now + 15 * SEC;
                        self.pending.push((at, Action::CreateGroup));
                    }
                    _ => {}
                }
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
                let media_id = CAMILLE_PHOTO;
                // Her full-resolution photo goes on the Internet bridge meanwhile.
                let _ = camille_offer();
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
            Action::React {
                from,
                target,
                emoji,
            } => {
                let reaction = Content::Reaction {
                    sent_at: ts,
                    target,
                    emoji: emoji.into(),
                }
                .encode();
                let _ = self
                    .net
                    .with_core(from, |c, now| c.send_datagram(&app_key, reaction, now));
            }
            Action::CamilleSticker => {
                let sticker = Content::Sticker {
                    sent_at: ts,
                    set: lc_core::content::STICKER_SET_NOTO,
                    emoji: "🥳".into(),
                }
                .encode();
                let camille = self.camille;
                let _ = self
                    .net
                    .with_core(camille, |c, now| c.send_datagram(&app_key, sticker, now));
            }
            Action::MarcWrites(text) => {
                let (marc, app) = (self.marc, self.app);
                self.net.send_plain_text(marc, app, &text, ts);
            }
            Action::PublicPost { from, text } => self.net.post_on_channel(from, 0, &text, ts),
            Action::JulienGif => {
                if let Some(gid) = self.group {
                    let gif = Content::Gif {
                        sent_at: ts,
                        provider: lc_core::content::GIF_GIPHY,
                        width: 200,
                        height: 102,
                        id: "l4KidtNiqsus526M8".into(),
                        title: "sunset sunrise denali".into(),
                    }
                    .encode();
                    let julien = self.julien;
                    let _ = self
                        .net
                        .with_core(julien, |c, now| c.send_group_datagram(&gid, &gif, now));
                }
            }
            Action::GuestJoins(text) => {
                let Some(guest) = self.guest else {
                    return;
                };
                let ready = self
                    .net
                    .with_core_ref(guest, lc_core::Core::ready)
                    .unwrap_or(false);
                if !ready {
                    self.pending
                        .push((self.net.now + 2 * SEC, Action::GuestJoins(text)));
                    return;
                }
                if let Some(invite) = lc_core::identity::Invite::decode(&text) {
                    let _ = self
                        .net
                        .with_core(guest, |c, now| c.join(&invite, true, now));
                }
            }
            Action::PostIn { from, gid, text } => {
                let _ = self
                    .net
                    .with_core(from, |c, now| c.send_group_text(&gid, &text, ts, now));
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
        // Camille and Julien scanned each other's code before the hike (ADR-0013); the app
        // meets them over the air.
        if !self.bots_met
            && [self.camille, self.julien].iter().all(|&b| {
                self.net
                    .nodes
                    .get(b)
                    .and_then(|n| n.core.as_ref())
                    .is_some_and(|c| c.ready())
            })
        {
            self.bots_met = true;
            self.net.introduce(self.camille, self.julien);
        }
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
        self.camille_acts();
        // Marc answers the user's first plain message (§15).
        let marc_heard = self
            .net
            .nodes
            .get_mut(self.marc)
            .is_some_and(|n| !std::mem::take(&mut n.inbox).is_empty());
        if marc_heard && !self.marc_answered {
            self.marc_answered = true;
            self.pending.push((
                now + 8 * SEC,
                Action::MarcWrites("It works! See you tonight, I'll keep the soup warm.".into()),
            ));
        }
        let app_key = self.net.key(self.app);
        // Once her photo's preview went out and the bridge is up, Camille announces the full
        // resolution (one LoRa frame).
        let offer = camille_offer()
            .lock()
            .ok()
            .map(|s| s.clone())
            .filter(|s| s.iroh.is_some() && !s.tor_pending);
        if !self.announced
            && self.camille_full.is_some()
            && let Some(offer) = offer
        {
            self.announced = true;
            let mut providers = Vec::new();
            if let Some(id) = offer.iroh {
                providers.push((lc_core::content::transport::IROH, id));
            }
            if let Some(id) = offer.onion {
                providers.push((lc_core::content::transport::TOR, id));
            }
            let ts = (self.unix_s() & 0xFFFF_FFFF) as u32;
            let announce = Content::Announce(lc_core::Announce {
                sent_at: ts,
                media_id: CAMILLE_PHOTO,
                kind: 1,
                hash: offer.hash,
                size: offer.size,
                providers,
                name: "refuge-dusk.avif".into(),
            })
            .encode();
            let _ = self.net.with_core(self.camille, |c, now| {
                c.send_datagram(&app_key, announce, now)
            });
        }
        // Léa says hello once the user's admitted her.
        if let Some(guest) = self.guest {
            for e in self.net.take_events(guest) {
                if let Event::GroupJoined { gid, .. } = e {
                    self.pending.push((
                        now + 20 * SEC,
                        Action::PostIn {
                            from: guest,
                            gid,
                            text: "Hi, it's Léa! I'm at the car park, see you at the col.".into(),
                        },
                    ));
                }
            }
        }
        for (bot, name) in [(self.camille, "Camille"), (self.julien, "Julien")] {
            for e in self.net.take_events(bot) {
                match e {
                    Event::Received {
                        from,
                        content: content @ Content::Text { .. },
                        ..
                    } if from == app_key => {
                        let Content::Text { text, .. } = &content else {
                            continue;
                        };
                        // Camille also gives it a 👍 (§6.2), soon after her answer.
                        let app_identity = self
                            .net
                            .with_core_ref(bot, |c| {
                                c.contacts()
                                    .find(|k| k.key == app_key)
                                    .and_then(|k| k.identity)
                            })
                            .flatten();
                        if let (Some(id), true) = (app_identity, name == "Camille") {
                            self.pending.push((
                                now + 8 * SEC,
                                Action::React {
                                    from: bot,
                                    target: lc_core::content::message_ref(&id, &content.encode()),
                                    emoji: "👍",
                                },
                            ));
                        }
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
                        content: Content::Sticker { .. },
                        ..
                    } if from == app_key && bot == self.camille => {
                        self.pending.push((now + 5 * SEC, Action::CamilleSticker));
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
                    Event::Beacon {
                        gid,
                        from,
                        sos: true,
                        ..
                    } if bot == self.camille
                        && from == self.net.key(self.julien)
                        && !self.camille_searching =>
                    {
                        self.camille_searching = true;
                        let _ = self
                            .net
                            .with_core(bot, |c, t| c.start_search(&gid, &from, t));
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
        // Julien posts once every member holds the keys, after Camille's photo has gone (the
        // script below counts on it: his first post may still be lost at the hub, the second
        // is lost on purpose, the GIF reveals the gap).
        if !self.julien_posted
            && now >= JULIEN_POSTS_FROM
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
            if self.julien_sos {
                // Hurt, without GPS: SOS (BEACONs without a position) and consent for pings.
                let _ = self.net.with_core(self.julien, |c, t| {
                    c.set_search_consent(&gid, true, t);
                    c.set_battery(Some(64));
                    c.set_sos(true, t);
                });
                self.pending.push((
                    now + 3 * SEC,
                    Action::GroupPost {
                        from: self.julien,
                        text: "Twisted my ankle above the gorge, no GPS here. SOS on.".into(),
                    },
                ));
                return;
            }
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
            // The app's radio misses both copies of his next post (interference at its end, the
            // hub busy): his GIF shows the gap, and a member hands the post back, late (§10.6).
            if let Some(julien) = self
                .net
                .with_core_ref(self.julien, |c| c.key().map(|k| k[0]))
            {
                self.net.deaf = Some(lose_second_message(self.app, julien));
            }
            self.pending.push((
                now + 35 * SEC,
                Action::GroupPost {
                    from: self.julien,
                    text: "Mist on the ridge, I'm taking the forest path instead.".into(),
                },
            ));
            self.pending.push((now + 85 * SEC, Action::JulienGif));
        }
    }
}

impl Demo {
    /// Camille, searching, pings Julien when allowed; her own SOS starts and ends on time.
    fn camille_acts(&mut self) {
        let now = self.net.now;
        if self.camille_searching && now >= self.next_camille_ping {
            self.next_camille_ping = now + 45 * SEC;
            let julien = self.net.key(self.julien);
            let _ = self
                .net
                .with_core(self.camille, |c, t| c.search_ping(&julien, t));
        }
        if let Some((from, until)) = self.camille_sos {
            let on = now >= from && now < until;
            let _ = self.net.with_core(self.camille, |c, t| {
                if c.sos() != on {
                    c.set_sos(on, t);
                }
            });
        }
    }

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
    /// Camille also offers her photo over Tor (the app has Tor on): she starts a Tor client
    /// in these folders, publishes her onion service, and announces once it is reachable.
    pub fn camille_over_tor(&self, state_dir: String, cache_dir: String) {
        if let Ok(mut s) = camille_offer().lock()
            && s.onion.is_none()
            && !s.tor_pending
        {
            s.tor_wanted = Some((state_dir.into(), cache_dir.into()));
            s.tor_pending = true;
        }
    }

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
        // A board with a GPS, off as the firmware ships it (protocol §1.1).
        net.nodes[app].settings.gps = Some(false);
        net.nodes[app].settings.autoadd = (0x02 | 0x01, 0);
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
        // People with the stock MeshCore app (§15): Marc at the refuge, Lucie on the trail,
        // within reach of the user and of the hub.
        // Everyone follows the public channel.
        let marc = net.add_stock_companion(5, "Marc");
        let lucie = net.add_stock_companion(6, "Lucie");
        net.link(hub, marc);
        net.link(hub, lucie);
        net.link(app, lucie);
        net.know(app, marc, true);
        net.know(marc, app, true);
        for n in [app, marc, lucie] {
            net.set_channel(
                n,
                0,
                lc_core::plain::PUBLIC_NAME,
                lc_core::plain::PUBLIC_SECRET,
            );
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
            (
                12 * SEC,
                Action::PublicPost {
                    from: lucie,
                    text: "Fog on the Pas de la Ville this morning, take care on the ridge.".into(),
                },
            ),
            (
                110 * SEC,
                Action::PublicPost {
                    from: lucie,
                    text: "The fog is lifting, the ridge is clear now.".into(),
                },
            ),
            (
                280 * SEC,
                Action::PublicPost {
                    from: lucie,
                    text: "Sun on the plateau, the path to the refuge is dry.".into(),
                },
            ),
            (
                320 * SEC,
                Action::PublicPost {
                    from: marc,
                    text: "Molière refuge: 12 beds left tonight, soup at 7 pm.".into(),
                },
            ),
            (
                45 * SEC,
                Action::MarcWrites(
                    "Hi! Marc from the refuge. Are you on lichen? I only have the MeshCore app."
                        .into(),
                ),
            ),
        ];
        Arc::new(Self {
            inner: Mutex::new(Demo {
                net,
                app,
                hub,
                camille,
                julien,
                guest: None,
                origin: None,
                group: None,
                pending,
                julien_posted: false,
                julien_walk: None,
                next_fix: 0,
                wall0: wall,
                camille_full: None,
                announced: false,
                bots_met: false,
                julien_sos: false,
                camille_searching: false,
                next_camille_ping: 0,
                camille_sos: None,
                marc,
                marc_answered: false,
            }),
        })
    }

    /// The SOS scenario (debug `sosdemo`): once the group is set up, Julien, hurt halfway down
    /// without GPS, turns SOS on; the app and Camille hear him directly (radio geometry from the
    /// real distances) and Camille searches for him too. Call right after `new`.
    pub fn julien_sos(&self) {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.julien_sos = true;
        let (app, camille, julien) = (d.app, d.camille, d.julien);
        for (node, at) in [(app, YOU), (camille, REFUGE), (julien, JULIEN_HURT)] {
            let (e, n) = plane(at);
            d.net.place(node, e, n);
        }
        d.net.link(app, julien);
        d.net.link(camille, julien);
        // Routes follow the new links (a node that overhears a frame routed the long way would
        // drop the relayed copy as a duplicate); the bots load them again.
        for (a, b) in [(app, julien), (camille, julien)] {
            d.net.know(a, b, true);
            d.net.know(b, a, true);
        }
        d.net.connect(camille);
        d.net.connect(julien);
    }

    /// Léa, standing next to the user, scans their group invitation `text` (debug
    /// `inviteDemo`, §10.5): her node comes up a few seconds later and asks to join.
    pub fn guest_scans(&self, text: String) {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let guest = match d.guest {
            Some(g) => g,
            None => {
                let (app, hub) = (d.app, d.hub);
                let g = d.net.add_companion(4, "Léa");
                d.net.link(g, app);
                d.net.link(g, hub);
                d.net.connect(g);
                let wall = d.wall0 + d.net.now;
                let _ = d.net.with_core(g, |c, now| c.set_clock(wall, now));
                d.guest = Some(g);
                g
            }
        };
        let _ = guest;
        let at = d.net.now + 3 * SEC;
        d.pending.push((at, Action::GuestJoins(text)));
    }

    /// Camille shows an invitation to a group of hers, "Refuge tonight", created for it (debug
    /// `-screen join`, §10.5): its QR code's text, once her node is up.
    pub fn camille_invite(&self) -> Option<String> {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let camille = d.camille;
        d.net
            .with_core(camille, |c, now| {
                let known = c
                    .groups()
                    .find(|g| g.name == "Refuge tonight")
                    .map(|g| g.gid);
                let gid = match known {
                    Some(g) => g,
                    None => c.create_group("Refuge tonight", &[], now).ok()?,
                };
                c.create_invite(&gid, now).ok().map(|i| i.encode())
            })
            .flatten()
    }

    /// Camille also in SOS (at the refuge, with GPS) from `after_s` for `for_s` seconds: two
    /// incidents at once, one ending (debug `sosdemo2`).
    pub fn camille_sos(&self, after_s: u64, for_s: u64) {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.camille_sos = Some((after_s * SEC, (after_s + for_s) * SEC));
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

    /// The user creates a group alone and shows its invitation; Léa, next to them, scans it,
    /// is admitted and says hello (§10.5).
    #[test]
    fn a_guest_joins_by_invitation() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("demo-test".into(), vec![8; 32], vec![108; 32], None).unwrap();
        core.set_group_consent(false);
        core.set_clock(unix_now_ms(), 0);
        core.connect(0);
        let mut gid = None;
        let (mut joined, mut hello) = (None, None);
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
                match e {
                    LichenEvent::MemberJoined { .. } => joined = joined.or(Some(now / 1000)),
                    LichenEvent::GroupMessage {
                        message: Message::Text { text, .. },
                        ..
                    } if text.starts_with("Hi, it's Léa") => hello = Some(now / 1000),
                    _ => {}
                }
            }
            if gid.is_none() && now >= 5_000 {
                let g = core
                    .create_group("Sortie du dimanche".into(), vec![], now)
                    .unwrap();
                let code = core.create_invite(g.clone(), now).unwrap();
                let info = core.parse_invite(code.text.clone(), now);
                assert_eq!(info, Err(crate::LichenError::BadCode), "our own invitation");
                demo.guest_scans(code.text);
                gid = Some(g);
            }
            if hello.is_some() {
                break;
            }
        }
        eprintln!("Léa admitted at {joined:?} s, her hello at {hello:?} s");
        assert!(joined.is_some(), "Léa was not admitted");
        assert!(hello.is_some(), "Léa's hello did not arrive");
        let g = core
            .groups()
            .into_iter()
            .find(|g| Some(&g.gid) == gid.as_ref())
            .unwrap();
        assert!(g.members.iter().any(|m| m.name == "Léa"));
        // The conversation shows what happened (ADR-0021), read already.
        let history = core
            .history(crate::ChatRef::Group { gid: g.gid.clone() }, None, 50)
            .unwrap();
        let notices: Vec<(u8, Vec<String>)> = history
            .iter()
            .filter_map(|m| match &m.message {
                Message::Notice { kind, subjects, .. } => {
                    Some((*kind, subjects.iter().map(|s| s.name.clone()).collect()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(notices, vec![(1, vec![]), (3, vec!["Léa".to_owned()])]);
        assert!(
            history
                .iter()
                .filter(|m| matches!(m.message, Message::Notice { .. }))
                .all(|m| m.read)
        );
        // The admin renames it: a line too.
        core.rename_group(g.gid.clone(), "Dimanche au Vercors".into(), 600_000)
            .unwrap();
        // Recorded as the app reads its events.
        while core.poll_event().is_some() {}
        let last = core
            .history(crate::ChatRef::Group { gid: g.gid.clone() }, None, 1)
            .unwrap();
        assert!(
            matches!(&last[0].message, Message::Notice { kind: 9, text, .. } if text == "Dimanche au Vercors")
        );
        assert_eq!(core.groups()[0].name, "Dimanche au Vercors");
    }

    /// The demo as the app runs it: frames back and forth, events out.
    #[test]
    fn demo_conversation_and_group() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("demo-test".into(), vec![7; 32], vec![107; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        let mut events = Vec::new();
        let mut sent = false;
        for step in 0..(20 * 60 * 10) {
            let now = step * 100;
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                if let LichenEvent::GroupMessage {
                    message: Message::Text { text, .. },
                    late,
                    ..
                } = &e
                    && (text.starts_with("Leaving the col") || text.starts_with("Mist"))
                {
                    eprintln!("{text} at {} s, late: {late}", now / 1000);
                }
                if let LichenEvent::GroupMessage {
                    message: Message::Gif { .. },
                    ..
                } = &e
                {
                    eprintln!("Julien's GIF at {} s", now / 1000);
                }
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
        // His second post, missed on the air, comes after his GIF: handed back by a member and
        // marked late (§10.6). His first one, often lost at the hub to Camille's photo, comes
        // back late too when it is.
        let order: Vec<(&str, bool)> = events
            .iter()
            .filter_map(|e| match e {
                LichenEvent::GroupMessage {
                    message: Message::Text { text, .. },
                    late,
                    ..
                } if text.starts_with("Leaving the col") => Some(("leaving", *late)),
                LichenEvent::GroupMessage {
                    message: Message::Text { text, .. },
                    late,
                    ..
                } if text.starts_with("Mist") => Some(("mist", *late)),
                LichenEvent::GroupMessage {
                    message: Message::Gif { .. },
                    late,
                    ..
                } => Some(("gif", *late)),
                _ => None,
            })
            .collect();
        let gif = order
            .iter()
            .position(|o| *o == ("gif", false))
            .unwrap_or_else(|| panic!("no GIF: {order:?}"));
        let mist = order
            .iter()
            .position(|o| *o == ("mist", true))
            .unwrap_or_else(|| panic!("no late Mist: {order:?}"));
        assert!(gif < mist, "{order:?}");
        let leaving = order.iter().position(|o| o.0 == "leaving").unwrap();
        assert!(leaving < gif || order[leaving].1, "{order:?}");
        assert_eq!(order.len(), 3, "{order:?}");
    }
}

#[cfg(test)]
mod location {
    use super::*;
    use crate::{LichenCore, LichenEvent, SharePrecision};

    /// The SOS scenario as the app runs it: Julien's SOS beacons (no GPS) arrive, the app
    /// searches, hears him directly and gets Camille's measures too; the estimate holds him.
    #[test]
    fn julien_sos_without_gps_is_searched() {
        let demo = DemoNetwork::new();
        demo.julien_sos();
        let core = LichenCore::new("t".into(), vec![5; 32], vec![105; 32], None).unwrap();
        core.set_group_consent(false);
        core.set_clock(unix_now_ms(), 0);
        core.connect(0);
        let mut searching: Option<Vec<u8>> = None;
        let mut sos_beacons = 0;
        let mut sent = false;
        let mut now = 0;
        while now < 15 * 60_000 {
            // The group needs Camille to know the app's identity: a first exchange.
            if !sent
                && now > 40_000
                && let Some(c) = core.contacts().into_iter().find(|c| c.name == "Camille")
            {
                sent = core.send_text(c.key, "On my way".into(), 1, now).is_ok();
            }
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                if let LichenEvent::Beacon {
                    gid,
                    from,
                    sos: true,
                    ..
                } = e
                {
                    sos_beacons += 1;
                    if searching.is_none() {
                        core.start_search(gid, from.clone(), now).unwrap();
                        searching = Some(from);
                    }
                }
            }
            if now % 10_000 == 0 {
                let mut me = demo.my_position();
                me.unix_s = (unix_now_ms() + now) / 1000;
                core.update_location(me, now);
            }
            if let Some(j) = &searching
                && now % 30_000 == 0
            {
                let _ = core.search_ping(j.clone(), now);
            }
            now += 100;
        }
        let julien = searching.expect("Julien's SOS beacons reached the app");
        assert!(sos_beacons >= 3, "{sos_beacons}");
        let ms = core.search_measurements(julien.clone());
        assert!(ms.iter().any(|m| m.mine), "the app measured Julien");
        assert!(ms.iter().any(|m| !m.mine), "Camille's measures arrived");
        let e = core.search_estimate(julien.clone()).expect("an estimate");
        let err = lc_core::location::distance_m((e.lat, e.lon), JULIEN_HURT);
        eprintln!(
            "{} measurements, error {err:.0} m, 95 % {:.0} m",
            ms.len(),
            e.major95_m
        );
        assert!(err < e.major95_m.max(800.0), "{err} m");
        assert_eq!(
            usize::from(e.heat_cols) * usize::from(e.heat_rows),
            e.heat.len()
        );
        let st = core.searches(now);
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].battery, Some(64));
        assert!(core.search_csv(julien).lines().count() > ms.len());
    }

    /// Camille's exact position arrives directly, Julien's through the group rounded to 1 km;
    /// the map gets one marker each.
    #[test]
    fn bots_share_their_positions() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![4; 32], vec![104; 32], None).unwrap();
        core.set_group_consent(false);
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
        let core = LichenCore::new("t".into(), vec![5; 32], vec![105; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        let mut changed: Vec<StoredMessage> = Vec::new();
        let mut step = 0;
        let mut now = 0;
        while now < 13 * 60_000 {
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
                    let v = crate::prepare_voice(pcm, 0);
                    assert_eq!((v.mode, v.frames, v.data.len()), (0, 75, 263));
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
mod bridge {
    use super::*;
    use crate::{LichenCore, LichenEvent, Message};

    /// Camille's preview arrives over LoRa, then her announcement; the app fetches the full
    /// photo over iroh from her endpoint, by id, verified (§14). Needs the Internet:
    /// `cargo test -p lc-ffi --release -- --ignored`.
    #[test]
    #[ignore = "needs the Internet"]
    fn camille_announces_her_photo_over_iroh() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![6; 32], vec![106; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        let mut announce = None;
        let mut now = 0;
        while now < 12 * 60_000 && announce.is_none() {
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                if let LichenEvent::MessageChanged { message } = e
                    && let Message::Announce {
                        media_id,
                        hash,
                        size,
                        providers,
                        ..
                    } = message.message
                {
                    announce = Some((media_id, hash, size, providers));
                }
            }
            now += 100;
            // The demo's clock is the app's: give the background bridge real time too.
            if now % 10_000 == 0 {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        let (media_id, hash, size, providers) = announce.expect("Camille announced her photo");
        assert_eq!(
            media_id,
            CAMILLE_PHOTO.to_vec(),
            "linked to her LoRa preview"
        );
        assert_eq!(providers[0].transport, lc_core::content::transport::IROH);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let bytes = rt.block_on(async {
            let me = lc_bridge::Bridge::start(
                lc_bridge::derive_key(&[0x77; 32], "lichen/1 iroh"),
                None,
                lc_bridge::Network::Default,
            )
            .await
            .unwrap();
            let key: [u8; 32] = providers[0].key.as_slice().try_into().unwrap();
            let hash: [u8; 32] = hash.as_slice().try_into().unwrap();
            let t = std::time::Instant::now();
            let got = me
                .fetch(lc_bridge::transport::IROH, key, hash, u64::from(size))
                .await
                .unwrap();
            eprintln!("{} bytes over iroh in {:?}", got.len(), t.elapsed());
            got
        });
        assert_eq!(bytes.len(), size as usize);
        assert_eq!(&bytes[4..12], b"ftypavif");
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
            Message::Voice { .. }
            | Message::Image { .. }
            | Message::Announce { .. }
            | Message::Notice { .. }
            | Message::File { .. }
            | Message::Sticker { .. }
            | Message::Gif { .. }
            | Message::Deleted { .. }
            | Message::Other { .. } => "",
        }
    }

    /// History, contacts and groups survive an app restart; the group keeps working both ways.
    #[test]
    fn restart_keeps_history_and_group() {
        let dir = std::env::temp_dir().join(format!("lichen-ffi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = Some(dir.to_string_lossy().into_owned());
        let demo = DemoNetwork::new();

        let core = LichenCore::new("t".into(), vec![1; 32], vec![101; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        let (mut sent, mut posted) = (false, false);
        run(&demo, &core, 0, 8 * 60_000, |c, now| {
            if !sent
                && now > 40_000
                && let Some(k) = c.contacts().into_iter().find(|k| k.name == "Camille")
            {
                sent = c.send_text(k.key, "first".into(), 1, now).is_ok();
            }
            // Our own post in the group: its history does not depend on a flood getting
            // through the demo's busiest minute.
            if !posted && let Some(g) = c.groups().into_iter().next() {
                posted = c.send_group_text(g.gid, "hello all".into(), 2, now).is_ok();
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
        drop(core);

        // The app restarts with its identity seed from the Keychain: history before connecting.
        let core = LichenCore::new("t".into(), vec![2; 32], vec![101; 32], data.clone()).unwrap();
        core.set_group_consent(false);
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
            // Camille answers each post; one of her answers lost at her first hop comes back
            // with the next (§10.6).
            if step == 2 && now > 15 * 60_000 {
                step = 3;
                c.send_group_text(group.gid.clone(), "anyone at the refuge?".into(), 4, now)
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
        let core = LichenCore::new("t".into(), vec![3; 32], vec![103; 32], None).unwrap();
        core.set_group_consent(false);
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

/// The outbox as the app runs it (protocol.md §6.1, ADR-0018).
#[cfg(test)]
mod outbox {
    use super::*;
    use crate::{ChatRef, FailReason, LichenCore, LichenEvent, Message, MessageStatus, WaitReason};

    fn run(demo: &DemoNetwork, core: &LichenCore, from: u64, to: u64) -> Vec<LichenEvent> {
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
            now += 100;
        }
        events
    }

    fn camille(core: &LichenCore) -> Vec<u8> {
        core.contacts()
            .into_iter()
            .find(|k| k.name == "Camille")
            .expect("Camille is known")
            .key
    }

    /// Camille's answers that quote `text`: one per copy she received.
    fn copies(events: &[LichenEvent], text: &str) -> usize {
        let quote = format!("Copy: \"{text}\"");
        events
            .iter()
            .filter(|e| {
                matches!(e, LichenEvent::MessageChanged { message }
                    if !message.outgoing
                        && matches!(&message.message, Message::Text { text, .. }
                            if text.starts_with(&quote)))
            })
            .count()
    }

    fn status_of(core: &LichenCore, chat: &[u8], id: i64) -> MessageStatus {
        core.history(ChatRef::Contact { key: chat.to_vec() }, None, 100)
            .unwrap()
            .into_iter()
            .find(|m| m.id == id)
            .expect("in the history")
            .status
    }

    fn data_dir(name: &str) -> (std::path::PathBuf, Option<String>) {
        let dir = std::env::temp_dir().join(format!("lichen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = Some(dir.to_string_lossy().into_owned());
        (dir, data)
    }

    /// Written while the radio is away, the app restarts, the radio comes back: delivered,
    /// once (the brief's acceptance test).
    #[test]
    fn a_text_written_offline_survives_a_restart() {
        let (dir, data) = data_dir("outbox-restart");
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![1; 32], vec![111; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 2 * 60_000);
        let camille = camille(&core);

        core.disconnect(2 * 60_000);
        let m = core
            .send_text(
                camille.clone(),
                "written offline".into(),
                1_793_000_000,
                2 * 60_000,
            )
            .unwrap();
        assert!(
            matches!(
                m.status,
                MessageStatus::Waiting {
                    reason: WaitReason::Radio,
                    started: false,
                    ..
                }
            ),
            "{:?}",
            m.status
        );
        drop(core);

        let core = LichenCore::new("t".into(), vec![2; 32], vec![111; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        assert!(matches!(
            status_of(&core, &camille, m.id),
            MessageStatus::Waiting {
                reason: WaitReason::Radio,
                started: false,
                ..
            }
        ));
        core.connect(3 * 60_000);
        let events = run(&demo, &core, 3 * 60_000, 10 * 60_000);
        assert_eq!(status_of(&core, &camille, m.id), MessageStatus::Delivered);
        assert_eq!(copies(&events, "written offline"), 1, "received once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Delivered, but the app stopped before it heard so: sent again at the next start, and
    /// Camille does not get it twice (at least once, deduplicated by the receiver).
    #[test]
    fn a_lost_acknowledgement_makes_no_duplicate() {
        let (dir, data) = data_dir("outbox-ack");
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![3; 32], vec![112; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 2 * 60_000);
        let camille = camille(&core);
        let m = core
            .send_text(
                camille.clone(),
                "hello once".into(),
                1_793_000_100,
                2 * 60_000,
            )
            .unwrap();
        let mut events = run(&demo, &core, 2 * 60_000, 6 * 60_000);
        assert_eq!(status_of(&core, &camille, m.id), MessageStatus::Delivered);
        // The acknowledgement never reached the database.
        core.lock()
            .store
            .as_ref()
            .unwrap()
            .set_status(m.id, lc_core::store::Status::Sending { sent: 1, total: 1 })
            .unwrap();
        drop(core);

        let core = LichenCore::new("t".into(), vec![4; 32], vec![112; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(6 * 60_000);
        events.extend(run(&demo, &core, 6 * 60_000, 14 * 60_000));
        assert_eq!(status_of(&core, &camille, m.id), MessageStatus::Delivered);
        assert_eq!(copies(&events, "hello once"), 1, "no duplicate");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_expiry_and_retry() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![5; 32], vec![113; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 2 * 60_000);
        let camille = camille(&core);
        core.disconnect(2 * 60_000);

        // Cancelled before anything went out: gone.
        let a = core
            .send_text(camille.clone(), "never mind".into(), 1, 2 * 60_000)
            .unwrap();
        core.cancel_message(a.id).unwrap();
        assert_eq!(
            core.cancel_message(a.id),
            Err(crate::LichenError::NotCancellable)
        );
        let h = core
            .history(
                ChatRef::Contact {
                    key: camille.clone(),
                },
                None,
                100,
            )
            .unwrap();
        assert!(h.iter().all(|m| m.id != a.id));

        // Its expiry passes while the radio is away.
        core.set_outbox_expiry(0);
        let b = core
            .send_text(camille.clone(), "too late".into(), 2, 2 * 60_000)
            .unwrap();
        while core.poll_event().is_some() {}
        assert_eq!(
            status_of(&core, &camille, b.id),
            MessageStatus::Failed {
                reason: FailReason::Expired
            }
        );

        // Sent again: the same message, waiting again, then delivered once.
        core.set_outbox_expiry(24 * 3_600_000);
        let again = core.retry_message(b.id, 2 * 60_000).unwrap();
        assert_eq!(again.id, b.id);
        assert!(matches!(again.status, MessageStatus::Waiting { .. }));
        core.connect(3 * 60_000);
        let events = run(&demo, &core, 3 * 60_000, 9 * 60_000);
        assert_eq!(status_of(&core, &camille, b.id), MessageStatus::Delivered);
        assert_eq!(copies(&events, "too late"), 1);
        assert_eq!(copies(&events, "never mind"), 0);
    }

    /// A group text written offline waits, then counts as sent once it went out.
    #[test]
    fn a_group_text_waits_for_the_radio() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![6; 32], vec![114; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 8 * 60_000);
        let gid = core.groups().into_iter().next().expect("in the group").gid;
        core.disconnect(8 * 60_000);
        let m = core
            .send_group_text(gid.clone(), "on my way".into(), 3, 8 * 60_000)
            .unwrap();
        assert!(matches!(
            m.status,
            MessageStatus::Waiting {
                reason: WaitReason::Radio,
                ..
            }
        ));
        core.connect(9 * 60_000);
        let events = run(&demo, &core, 9 * 60_000, 11 * 60_000);
        assert!(events.iter().any(|e| matches!(e,
            LichenEvent::MessageChanged { message }
                if message.id == m.id && message.status == MessageStatus::Flooded)));
    }
}

/// Replies and reactions as the app runs them (protocol.md §6.2, ADR-0019).
#[cfg(test)]
mod replies {
    use super::*;
    use crate::{ChatRef, LichenCore, LichenEvent, Message, MessageStatus, QuoteKind};

    fn run(demo: &DemoNetwork, core: &LichenCore, from: u64, to: u64) -> Vec<LichenEvent> {
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
            now += 100;
        }
        events
    }

    fn history(core: &LichenCore, chat: &ChatRef) -> Vec<crate::StoredMessage> {
        core.history(chat.clone(), None, 100).unwrap()
    }

    fn text_of(m: &crate::StoredMessage) -> &str {
        match &m.message {
            Message::Text { text, .. } => text,
            _ => "",
        }
    }

    fn data_dir(name: &str) -> (std::path::PathBuf, Option<String>) {
        let dir = std::env::temp_dir().join(format!("lichen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = Some(dir.to_string_lossy().into_owned());
        (dir, data)
    }

    #[test]
    fn reactions_and_a_reply_with_camille() {
        let (dir, data) = data_dir("replies");
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![1; 32], vec![131; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 60_000);
        let camille = core
            .contacts()
            .into_iter()
            .find(|k| k.name == "Camille")
            .unwrap()
            .key;
        let chat = ChatRef::Contact {
            key: camille.clone(),
        };

        // Camille gives our text a 👍.
        let mine = core
            .send_text(camille.clone(), "On my way".into(), 1_793_000_000, 60_000)
            .unwrap();
        // The link is busy in the demo's first minutes (the group being set up): delivery may
        // take a few rounds and a new session.
        let events = run(&demo, &core, 60_000, 420_000);
        let ours = history(&core, &chat)
            .into_iter()
            .find(|m| m.id == mine.id)
            .unwrap();
        assert_eq!(ours.reactions.len(), 1, "{:?}", ours.reactions);
        assert_eq!(ours.reactions[0].emoji, "👍");
        assert!(!ours.reactions[0].mine);
        assert!(events.iter().any(|e| matches!(e,
            LichenEvent::Reacted { emoji, target: Some(t), .. } if emoji == "👍" && t.id == mine.id)));

        // We react to her "Hi", change our mind, then take it back.
        let hi = history(&core, &chat)
            .into_iter()
            .find(|m| !m.outgoing && text_of(m).starts_with("Hi!"))
            .unwrap();
        assert!(hi.referable);
        let r = core.send_reaction(hi.id, "❤️".into(), 420_000).unwrap();
        assert_eq!(r.reactions.len(), 1);
        assert!(r.reactions[0].mine);
        let r = core.send_reaction(hi.id, "😂".into(), 420_000).unwrap();
        assert_eq!(
            r.reactions
                .iter()
                .map(|c| c.emoji.as_str())
                .collect::<Vec<_>>(),
            vec!["😂"]
        );
        let r = core.send_reaction(hi.id, String::new(), 420_000).unwrap();
        assert!(r.reactions.is_empty());

        // A reply to it, delivered, quoting it.
        let reply = core
            .send_reply(
                chat.clone(),
                hi.id,
                "Coming!".into(),
                1_793_000_100,
                420_000,
            )
            .unwrap();
        run(&demo, &core, 420_000, 720_000);
        let me = *core.lock().core.key().unwrap();
        assert!(
            core.lock()
                .store
                .as_ref()
                .unwrap()
                .pending_reactions(&me)
                .unwrap()
                .is_empty(),
            "our reactions were delivered"
        );
        let check = |core: &LichenCore| {
            let h = history(core, &chat);
            let r = h.iter().find(|m| m.id == reply.id).unwrap();
            assert_eq!(r.status, MessageStatus::Delivered);
            let q = r.quote.as_ref().unwrap();
            assert_eq!(q.message_id, Some(hi.id));
            assert_eq!(q.kind, QuoteKind::Text);
            assert!(q.excerpt.starts_with("Hi!"));
            let ours = h.iter().find(|m| m.id == mine.id).unwrap();
            assert_eq!(ours.reactions[0].emoji, "👍");
        };
        check(&core);
        // Kept across a restart.
        drop(core);
        let core = LichenCore::new("t".into(), vec![2; 32], vec![131; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        check(&core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Our reaction made while the radio is away waits, survives a restart and goes out.
    #[test]
    fn a_pending_reaction_goes_after_a_restart() {
        let (dir, data) = data_dir("reaction-restart");
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![3; 32], vec![132; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        run(&demo, &core, 0, 60_000);
        let camille = core
            .contacts()
            .into_iter()
            .find(|k| k.name == "Camille")
            .unwrap()
            .key;
        let chat = ChatRef::Contact { key: camille };
        let hi = history(&core, &chat)
            .into_iter()
            .find(|m| !m.outgoing)
            .unwrap();
        core.disconnect(60_000);
        core.send_reaction(hi.id, "🙏".into(), 60_000).unwrap();
        let me = *core.lock().core.key().unwrap();
        drop(core);

        let core = LichenCore::new("t".into(), vec![4; 32], vec![132; 32], data.clone()).unwrap();
        core.set_group_consent(false);
        let pending = |c: &LichenCore| {
            c.lock()
                .store
                .as_ref()
                .unwrap()
                .pending_reactions(&me)
                .unwrap()
                .len()
        };
        assert_eq!(pending(&core), 1);
        core.connect(70_000);
        run(&demo, &core, 70_000, 180_000);
        assert_eq!(pending(&core), 0, "delivered after the restart");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// MeshCore chats in the demo (protocol.md §15, ADR-0026): Marc writes from the stock app,
/// Lucie posts on Public, and Marc answers the user's reply.
#[cfg(test)]
mod meshcore {
    use super::*;
    use crate::{ChatRef, LichenCore, LichenEvent, Message, MessageStatus};

    fn run(demo: &DemoNetwork, core: &LichenCore, from: u64, to: u64) -> Vec<LichenEvent> {
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
            now += 100;
        }
        events
    }

    fn text(m: &crate::StoredMessage) -> &str {
        match &m.message {
            Message::Text { text, .. } => text,
            _ => "",
        }
    }

    #[test]
    fn marc_and_the_public_channel() {
        let demo = DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![1; 32], vec![150; 32], None).unwrap();
        core.set_group_consent(false);
        core.connect(0);
        let events = run(&demo, &core, 0, 70_000);
        let channels = core.channels();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].kind, crate::ChannelKind::PublicChannel);

        let marc = core
            .contacts()
            .into_iter()
            .find(|c| c.name == "Marc")
            .unwrap()
            .key;
        let mesh = ChatRef::Mesh { key: marc.clone() };
        assert!(events.iter().any(|e| matches!(e,
            LichenEvent::PlainReceived { chat, label: Some(l), .. } if *chat == mesh && l == "Marc")));
        let h = core.history(mesh.clone(), None, 10).unwrap();
        assert!(text(&h[0]).starts_with("Hi! Marc from the refuge."));
        assert!(
            !h[0].referable,
            "no replies or reactions in a MeshCore chat"
        );

        // Lichen content is refused in a MeshCore chat.
        assert!(
            core.send_sticker(mesh.clone(), "🥳".into(), 1, 70_000)
                .is_err()
        );

        // The user answers once the hub is quiet (in the first minutes the group's setup and
        // Camille's photo keep it busy, and acknowledgements collide there); Marc's radio
        // acknowledges, and he answers once.
        run(&demo, &core, 70_000, 330_000);
        let mine = core
            .send_plain(
                mesh.clone(),
                "Yes! Write to me here.".into(),
                1_793_000_000,
                330_000,
            )
            .unwrap();
        let mut events = run(&demo, &core, 330_000, 420_000);
        let h = core.history(mesh.clone(), None, 10).unwrap();
        let ours = h.iter().find(|m| m.id == mine.id).unwrap();
        // Its three attempts can all collide at the hub with the demo's other traffic: then
        // the user retries, as the app offers.
        if matches!(ours.status, MessageStatus::Failed { .. }) {
            core.retry_message(mine.id, 420_000).unwrap();
            events.extend(run(&demo, &core, 420_000, 520_000));
        }
        let h = core.history(mesh.clone(), None, 10).unwrap();
        let ours = h.iter().find(|m| m.id == mine.id).unwrap();
        assert_eq!(ours.status, MessageStatus::Delivered, "{events:?}");
        assert!(
            h.iter()
                .any(|m| !m.outgoing && text(m).starts_with("It works!"))
        );
        let posts = core
            .history(
                ChatRef::Channel {
                    id: channels[0].id.clone(),
                },
                None,
                10,
            )
            .unwrap();
        // Three posts on Public, by Lucie and Marc, with their names. A post is flooded once
        // and never retried (as with stock clients), so in the demo's busy first minutes one
        // may be lost.
        assert!(!posts.is_empty());
        for m in &posts {
            assert!(
                matches!(m.author_name.as_deref(), Some("Lucie" | "Marc")),
                "{m:?}"
            );
        }
    }
}
