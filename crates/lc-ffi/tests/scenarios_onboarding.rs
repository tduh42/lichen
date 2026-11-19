//! Onboarding and identity (docs/testing.md §1, ON-1 to ON-7), as the apps call the core.

mod common;

use common::{Sim, TempDir, contact_chat, new_core};
use lc_ffi::{
    ChatRef, LichenCore, LichenError, LichenEvent, MessageStatus, fingerprint_of, qr_code,
    seed_from_phrase,
};

/// ON-1: the first launch makes an identity; its 24 words come back to the same seed; Messages
/// is empty and no radio is known yet (the radio prompt).
#[test]
fn on_1_first_launch_creates_an_identity() {
    let dir = TempDir::new("on1");
    let seed = vec![61u8; 32];
    let core = LichenCore::new("t".into(), vec![1; 32], seed.clone(), Some(dir.path())).unwrap();
    assert!(core.storage_ok());
    let key = core.key().expect("an identity at once");
    assert_eq!(key.len(), 32);
    let words = core.recovery_phrase();
    assert_eq!(words.len(), 24);
    assert_eq!(seed_from_phrase(words).unwrap(), seed);
    let fp = core.fingerprint();
    assert_eq!(fp.len(), 6);
    assert!(
        fp.iter()
            .all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit()))
    );
    assert_eq!(fingerprint_of(key).unwrap(), fp);
    // Messages: nothing yet, and no radio (the app shows the radio prompt).
    assert!(core.conversations().is_empty());
    assert!(core.contacts().is_empty());
    assert!(core.groups().is_empty());
    assert!(core.saved_radios().is_empty());
    assert_eq!(core.node_key(), None);
    // No card before a radio: the card names our node.
    assert_eq!(core.contact_card("Me".into()), Err(LichenError::NoIdentity));
    // A seed the Keychain mangled is refused, not used.
    assert!(LichenCore::new("t".into(), vec![1; 32], vec![61; 31], None).is_err());
}

/// ON-2: after onboarding, a restart finds the same identity (key, fingerprint, words) and its
/// history, without onboarding again.
#[test]
fn on_2_restart_keeps_the_identity() {
    let mut sim = Sim::persistent(62, "on2");
    sim.until_event("Connected", 10, |e| {
        matches!(e, LichenEvent::Connected { .. }).then_some(())
    });
    let (key, fp, words) = (
        sim.core.key(),
        sim.core.fingerprint(),
        sim.core.recovery_phrase(),
    );
    let camille = sim.camille();
    let hello = sim.until_some("Camille's hello", 60, |c| {
        c.conversations()
            .into_iter()
            .find(|v| v.chat == contact_chat(&camille))
    });
    sim.restart(1_000);
    assert_eq!(sim.core.key(), key);
    assert_eq!(sim.core.fingerprint(), fp);
    assert_eq!(sim.core.recovery_phrase(), words);
    assert!(sim.core.storage_ok());
    // What the first run kept is there before the radio answers.
    let again = sim
        .conversation(&hello.chat)
        .expect("the conversation kept");
    assert_eq!(again.last.id, hello.last.id);
    assert_eq!(again.name, "Camille");
    // The radio comes back under the same identity: Camille needs no new introduction.
    sim.until_event("Connected again", 10, |e| {
        matches!(e, LichenEvent::Connected { .. }).then_some(())
    });
    let c = common::find_contact(&sim.core, "Camille").unwrap();
    assert_eq!(c.identity, camille.identity);
}

/// ON-3: the recovery phrase gives back this identity; a phrase with a word changed, missing
/// or misspelt is refused (the confirmation fails).
#[test]
fn on_3_recovery_phrase_matches_and_a_wrong_one_is_refused() {
    let core = new_core(63, 1, None);
    let words = core.recovery_phrase();
    let seed = seed_from_phrase(words.clone()).unwrap();
    assert_eq!(seed, vec![63; 32]);
    // Restored on another phone: the same identity.
    let restored = LichenCore::new("t".into(), vec![9; 32], seed, None).unwrap();
    assert_eq!(restored.key(), core.key());
    assert_eq!(restored.fingerprint(), core.fingerprint());
    // Typed with capitals and spaces: still the same.
    let sloppy: Vec<String> = words
        .iter()
        .map(|w| format!("  {} ", w.to_uppercase()))
        .collect();
    assert_eq!(seed_from_phrase(sloppy).unwrap(), vec![63; 32]);
    // Two words swapped: the checksum refuses it.
    let mut swapped = words.clone();
    let other = words.iter().position(|w| *w != words[0]).unwrap();
    swapped.swap(0, other);
    assert_eq!(seed_from_phrase(swapped), Err(LichenError::BadCode));
    // A word missing, a word that is not in the list.
    assert_eq!(
        seed_from_phrase(words[..23].to_vec()),
        Err(LichenError::BadCode)
    );
    let mut typo = words.clone();
    typo[5] = "lichenx".into();
    assert_eq!(seed_from_phrase(typo), Err(LichenError::BadCode));
    assert_eq!(seed_from_phrase(vec![]), Err(LichenError::BadCode));
}

/// ON-4: my card names me and my node; its QR code carries exactly the card; another phone
/// that scans it gets my identity, verified.
#[test]
fn on_4_my_card_and_its_qr() {
    let mut sim = Sim::new(64);
    sim.until_event("Connected", 10, |e| {
        matches!(e, LichenEvent::Connected { .. }).then_some(())
    });
    let card = sim.core.contact_card("Morgan".into()).unwrap();
    assert!(card.starts_with("lichen:1:"), "{card}");
    let decoded = lc_core::identity::Card::decode(&card).unwrap();
    assert_eq!(decoded.name, "Morgan");
    assert_eq!(Some(decoded.identity.to_vec()), sim.core.key());
    assert_eq!(Some(decoded.node.to_vec()), sim.core.node_key());
    // The QR code the screen draws is the card's text.
    let qr = qr_code(card.clone()).unwrap();
    let reference =
        qrcode::QrCode::with_error_correction_level(card.as_bytes(), qrcode::EcLevel::M).unwrap();
    assert_eq!(qr.width as usize, reference.width());
    let modules: Vec<bool> = reference
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    assert_eq!(qr.dark, modules, "the QR code of the card's text");
    // Scanned by someone else: we become their contact, verified, under the card's name.
    let other = new_core(164, 2, None);
    let me = other.add_contact_card(card.clone(), true).unwrap();
    assert_eq!(Some(me.key.clone()), sim.core.node_key());
    assert_eq!(me.identity, sim.core.key());
    assert!(me.verified);
    assert_eq!(me.name, "Morgan");
    // Our own card is not a contact of ours, and the app can say why (B-and-10).
    assert!(sim.core.own_code(card.clone()));
    assert!(!other.own_code(card.clone()));
    assert!(!sim.core.own_code("lichen:1:nope".into()));
    assert_eq!(
        sim.core.add_contact_card(card, true),
        Err(LichenError::BadCode)
    );
}

/// ON-5: what each identity or database problem screen relies on. The database cannot be
/// opened: `storage_ok` is false and nothing is sent. A database encrypted under a key that
/// is lost: `encrypted_database` says so, `discard_database` is the way out, and the identity
/// starts again with an empty history. A key the Keychain cannot give now: no database is
/// opened rather than a clear one.
#[test]
fn on_5_identity_and_database_problems() {
    let seed = vec![65u8; 32];
    // Storage: the folder is a file.
    let dir = TempDir::new("on5");
    let blocked = dir.0.join("not-a-folder");
    std::fs::write(&blocked, b"x").unwrap();
    let core = LichenCore::new(
        "t".into(),
        vec![1; 32],
        seed.clone(),
        Some(blocked.to_string_lossy().into_owned()),
    )
    .unwrap();
    assert!(!core.storage_ok());
    assert!(core.key().is_some(), "the identity is not at fault");
    assert_eq!(
        core.send_text(vec![9; 32], "hi".into(), 1, 0).err(),
        Some(LichenError::Storage)
    );

    // A database whose key is gone: it is not plain SQLite.
    let lost = TempDir::new("on5-lost");
    let first = LichenCore::new("t".into(), vec![1; 32], seed.clone(), Some(lost.path())).unwrap();
    assert!(first.storage_ok());
    first
        .set_archived(ChatRef::Contact { key: vec![9; 32] }, true)
        .unwrap();
    drop(first);
    assert_eq!(
        lc_ffi::encrypted_database(lost.path(), seed.clone()),
        Ok(false),
        "a plain database"
    );
    let file = std::fs::read_dir(&lost.0)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "sqlite"))
        .expect("the identity's database");
    // What SQLCipher leaves on disk: no SQLite header.
    std::fs::write(&file, vec![0xA5; 8192]).unwrap();
    assert_eq!(
        lc_ffi::encrypted_database(lost.path(), seed.clone()),
        Ok(true)
    );
    let stuck = LichenCore::new("t".into(), vec![1; 32], seed.clone(), Some(lost.path())).unwrap();
    assert!(!stuck.storage_ok());
    drop(stuck);
    // The way out: start again with an empty history, same identity.
    lc_ffi::discard_database(lost.path(), seed.clone()).unwrap();
    assert_eq!(
        lc_ffi::encrypted_database(lost.path(), seed.clone()),
        Ok(false)
    );
    let fresh = LichenCore::new("t".into(), vec![1; 32], seed.clone(), Some(lost.path())).unwrap();
    assert!(fresh.storage_ok());
    assert!(fresh.chat_prefs().is_empty(), "an empty history");
    assert_eq!(fresh.key(), core.key());
    // Discarding twice is not an error.
    drop(fresh);
    lc_ffi::discard_database(lost.path(), seed.clone()).unwrap();
    lc_ffi::discard_database(lost.path(), seed.clone()).unwrap();

    // The database key cannot be read now (empty): no database, not a clear one.
    let wait = TempDir::new("on5-key");
    let core = LichenCore::with_database_key(
        "t".into(),
        vec![1; 32],
        seed.clone(),
        Some(wait.path()),
        Some(vec![]),
    )
    .unwrap();
    assert!(!core.storage_ok());
    assert!(
        std::fs::read_dir(&wait.0).unwrap().next().is_none(),
        "nothing written"
    );
    // A seed of the wrong length (a corrupt Keychain entry) is refused.
    assert_eq!(
        lc_ffi::encrypted_database(wait.path(), vec![1; 5]),
        Err(LichenError::InvalidKey)
    );
}

/// ON-6: verifying Camille: the safety number is the same on both sides; matching numbers mark
/// her verified, a wrong identity verifies nothing, and "they differ" takes it back.
#[test]
fn on_6_verify_a_contact() {
    let mut sim = Sim::new(66);
    let camille = sim.camille();
    assert!(!camille.verified);
    let id = camille.identity.clone().unwrap();
    let mine = sim.core.safety_number(camille.key.clone()).unwrap();
    assert_eq!(mine.len(), 12);
    assert!(
        mine.iter()
            .all(|g| g.len() == 5 && g.chars().all(|c| c.is_ascii_digit()))
    );
    assert_eq!(sim.core.safety_number_with(id.clone()).unwrap(), mine);
    // Camille's phone computes it from her side.
    let me: [u8; 32] = sim.core.key().unwrap().try_into().unwrap();
    let hers: [u8; 32] = id.clone().try_into().unwrap();
    assert_eq!(
        lc_core::identity::safety_number(&hers, &me).to_vec(),
        mine,
        "the same number on both phones"
    );
    // Another identity's number is not hers: nothing verified.
    assert!(
        !sim.core
            .set_verified(camille.key.clone(), vec![3; 32], true)
            .unwrap()
    );
    assert!(!common::find_contact(&sim.core, "Camille").unwrap().verified);
    assert!(
        sim.core
            .set_verified(camille.key.clone(), id.clone(), true)
            .unwrap()
    );
    let after = common::find_contact(&sim.core, "Camille").unwrap();
    assert!(after.verified);
    assert_eq!(after.identity, Some(id.clone()));
    // Both phones see the same number between two apps too.
    let other = new_core(166, 3, None);
    let card = sim.core.contact_card("Me".into()).unwrap();
    let me_there = other.add_contact_card(card, true).unwrap();
    let other_card = lc_core::identity::Card {
        identity: other.key().unwrap().try_into().unwrap(),
        node: [0x66; 32],
        name: "Other".into(),
    }
    .encode();
    let them = sim.core.add_contact_card(other_card, true).unwrap();
    assert_eq!(
        sim.core.safety_number(them.key).unwrap(),
        other.safety_number(me_there.key).unwrap()
    );
    // Numbers that differ: not verified any more.
    assert!(
        sim.core
            .set_verified(camille.key.clone(), id, false)
            .unwrap()
    );
    assert!(!common::find_contact(&sim.core, "Camille").unwrap().verified);
}

/// ON-7: a card pasted adds an unverified contact, scanned a verified one; either way a
/// conversation can start. Camille's card, scanned, reaches her.
#[test]
fn on_7_add_a_contact_by_card() {
    let mut sim = Sim::new(67);
    sim.until_event("Connected", 10, |e| {
        matches!(e, LichenEvent::Connected { .. }).then_some(())
    });
    // Someone the radio does not know yet, pasted.
    let stranger = lc_core::identity::Card {
        identity: [0x71; 32],
        node: [0x72; 32],
        name: "Alex".into(),
    }
    .encode();
    let alex = sim.core.add_contact_card(stranger.clone(), false).unwrap();
    assert_eq!(alex.name, "Alex");
    assert_eq!(alex.identity, Some(vec![0x71; 32]));
    assert!(!alex.verified);
    assert!(alex.person);
    assert!(
        sim.drain()
            .iter()
            .any(|e| matches!(e, LichenEvent::ContactsUpdated))
    );
    assert!(common::find_contact(&sim.core, "Alex").is_some());
    // A conversation can start: the text waits for a path.
    let m = sim
        .core
        .send_text(alex.key.clone(), "Hello Alex".into(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(matches!(m.status, MessageStatus::Waiting { .. }), "{m:?}");
    assert!(sim.conversation(&contact_chat(&alex)).is_some());
    // The same card scanned in person: verified.
    let alex = sim.core.add_contact_card(stranger, true).unwrap();
    assert!(alex.verified);
    // Garbage is refused.
    assert_eq!(
        sim.core.add_contact_card("lichen:1:nope".into(), false),
        Err(LichenError::BadCode)
    );

    // Camille's card, scanned before she wrote: verified at once, and she gets our text.
    let camille = sim.contact("Camille");
    let identity = sim.until_some("Camille's identity", 120, |c| {
        common::find_contact(c, "Camille").and_then(|k| k.identity)
    });
    let card = lc_core::identity::Card {
        identity: identity.try_into().unwrap(),
        node: camille.key.clone().try_into().unwrap(),
        name: "Camille".into(),
    }
    .encode();
    let c = sim.core.add_contact_card(card, true).unwrap();
    assert!(c.verified);
    let m = sim
        .core
        .send_text(c.key.clone(), "Scanned you".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&contact_chat(&c), m.id, 120, |s| {
        *s == MessageStatus::Delivered
    });
}

/// The app's core on a small network of its own (`lc_core::fakenet`), pumped as `Sim` pumps
/// the demo: Léa, whose radio has never heard ours.
struct Small {
    net: lc_core::fakenet::Net,
    app: usize,
    lea: usize,
    core: std::sync::Arc<LichenCore>,
    now: u64,
    wall0: u64,
    /// Every event polled.
    events: Vec<LichenEvent>,
}

impl Small {
    /// Léa next to us (`beside`), or one repeater away.
    fn new(beside: bool) -> Self {
        let radio = lc_meshcore::airtime::LoRaParams::meshcore(8, 62_500, 5).unwrap();
        let mut net = lc_core::fakenet::Net::new(radio, 0, 7);
        let app = net.add_external_companion(1, "ThinkNode M5");
        let hub = net.add_repeater(10, "hub");
        let lea = net.add_companion(4, "Léa");
        net.link(app, hub);
        net.link(hub, lea);
        if beside {
            net.link(app, lea);
        }
        let wall0 = common::unix_now_ms();
        net.connect(lea);
        let _ = net.with_core(lea, |c, now| c.set_clock(wall0, now));
        let core = new_core(68, 168, None);
        core.set_clock(wall0, 0);
        core.connect(0);
        let mut s = Self {
            net,
            app,
            lea,
            core,
            now: 0,
            wall0,
            events: Vec::new(),
        };
        s.run_until("both ready", 20, |s| {
            s.core.node_key().is_some()
                && s.net.with_core_ref(s.lea, lc_core::Core::ready) == Some(true)
        });
        s
    }

    fn step(&mut self) {
        let now = self.now;
        if now > self.net.now {
            self.net.run_until(now, |_| false);
        }
        for f in self.net.external_read(self.app) {
            self.core.on_companion_frame(f, now);
        }
        self.core.set_clock(self.wall0 + now, now);
        self.core.tick(now);
        while let Some(f) = self.core.poll_companion(now) {
            self.net.external_write(self.app, &f);
        }
        while let Some(e) = self.core.poll_event() {
            self.events.push(e);
        }
        self.now += common::STEP_MS;
    }

    fn run_until(&mut self, what: &str, limit_s: u64, mut done: impl FnMut(&mut Self) -> bool) {
        let end = self.now + limit_s * 1000;
        while self.now < end {
            if done(self) {
                return;
            }
            self.step();
        }
        panic!("{what}: not within {limit_s} s");
    }

    /// Léa's contact card, as her app shows it.
    fn lea_card(&self) -> String {
        let identity = self
            .net
            .with_core_ref(self.lea, |c| c.key().copied())
            .flatten()
            .unwrap();
        lc_core::identity::Card {
            identity,
            node: self.net.key(self.lea),
            name: "Léa".into(),
        }
        .encode()
    }
}

/// ON-7 (B-ios-1): a contact added from a card alone, whose radio has never heard ours, can be
/// written to at once: scanned beside them, pasted from one repeater away, or scanned while our
/// radio was away (our advert goes when it connects).
#[test]
fn on_7_a_card_one_way_can_be_written_to_at_once() {
    for (beside, scanned, away) in [
        (true, true, false),
        (false, false, false),
        (true, true, true),
    ] {
        let mut s = Small::new(beside);
        if away {
            s.core.disconnect(s.now);
        }
        let lea = s.core.add_contact_card(s.lea_card(), scanned).unwrap();
        if away {
            let back = s.now + 5_000;
            s.run_until("a while away", 10, |s| s.now >= back);
            s.core.connect(s.now);
            let before = s.events.len();
            s.run_until("connected again", 10, |s| {
                s.events[before..]
                    .iter()
                    .any(|e| matches!(e, LichenEvent::Connected { .. }))
            });
        }
        assert_eq!(lea.verified, scanned);
        let m = s
            .core
            .send_text(
                lea.key.clone(),
                "Hello Léa".into(),
                ((s.wall0 + s.now) / 1000) as u32,
                s.now,
            )
            .unwrap();
        let chat = contact_chat(&lea);
        let mut status = m.status.clone();
        s.run_until("the first text settled", 300, |s| {
            status = s
                .core
                .history(chat.clone(), None, 10)
                .unwrap()
                .into_iter()
                .find(|x| x.id == m.id)
                .unwrap()
                .status;
            matches!(
                status,
                MessageStatus::Delivered | MessageStatus::Failed { .. }
            )
        });
        assert_eq!(
            status,
            MessageStatus::Delivered,
            "scanned {scanned}, beside {beside}"
        );
    }
}
