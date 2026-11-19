//! Settings (docs/testing.md §9: ST-1, ST-3, ST-4, ST-8). The app lock (ST-2), About (ST-5),
//! French (ST-6), dark mode (ST-7) and accessibility (ST-9) are the apps' own; Tor starting
//! and stopping (ST-8) needs the Internet: `cargo test -p lc-bridge --release -- --ignored tor`.

mod common;

use common::{Sim, TempDir, contact_chat, find_contact, new_core, texts};
use lc_ffi::{
    ChatRef, LichenBridge, LichenError, OnionStatus, ReportApp, ReportLine, onion_address,
};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// ST-1: Settings › Privacy: who is blocked is listed and stays blocked after a restart; the
/// radio keeps our position out of its adverts unless asked.
#[test]
fn st_1_privacy_settings_applied_and_kept() {
    let mut sim = Sim::persistent(161, "st1");
    let marc = sim.contact("Marc");
    sim.core.block(marc.key.clone(), sim.now).unwrap();
    assert_eq!(sim.core.blocked().len(), 1);
    sim.until("the radio's settings", 10, |c| {
        c.radio_settings().autoadd_mask.is_some()
    });
    let s = sim.core.radio_settings();
    assert!(!s.position_in_adverts, "off unless the user turns it on");
    assert_eq!(s.telemetry.location, 0, "no location telemetry");
    sim.restart(1_000);
    let b = sim.core.blocked();
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].name, "Marc");
    assert!(sim.core.is_blocked(marc.key.clone()));
    // His message at 45 s does not come.
    sim.run_to(120_000);
    let mesh = ChatRef::Mesh {
        key: marc.key.clone(),
    };
    assert!(sim.history(&mesh).is_empty());
    assert_eq!(
        sim.core
            .send_plain(mesh, "?".into(), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::Blocked)
    );
}

/// ST-3: a backup exported on one phone and restored on another with the 24 words: the same
/// identity, contacts, group, messages and the app's settings; wrong words are refused.
#[test]
fn st_3_backup_export_and_restore() {
    let mut sim = Sim::persistent(163, "st3");
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "Backed up".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.until_status(&chat, m.id, 300, |s| *s == lc_ffi::MessageStatus::Delivered);
    let group = sim.rando();
    sim.core
        .rename_contact(camille.key.clone(), Some("Cam".into()))
        .unwrap();
    let before = texts(&sim.history(&chat));
    let words = sim.core.recovery_phrase();
    let out = TempDir::new("st3-out");
    let file = out.0.join("lichen.backup").display().to_string();
    let settings = br#"{"theme":"dark","radioDetails":true}"#.to_vec();
    let summary = sim
        .core
        .export_backup(file.clone(), settings.clone())
        .unwrap();
    assert!(summary.bytes > 0 && summary.messages >= before.len() as u64);

    // The new phone: a core of its own, then the backup into its folder.
    let new_phone = TempDir::new("st3-new");
    let first = new_core(200, 1, None);
    let mut wrong = words.clone();
    wrong.swap(0, 1);
    assert!(
        first
            .restore_backup(file.clone(), wrong, new_phone.path())
            .is_err()
    );
    let junk = out.0.join("junk").display().to_string();
    std::fs::write(&junk, b"nothing to see").unwrap();
    assert_eq!(
        first.restore_backup(junk, words.clone(), new_phone.path()),
        Err(LichenError::NotABackup)
    );
    let restored = first.restore_backup(file, words, new_phone.path()).unwrap();
    assert_eq!(restored.identity_seed, vec![163; 32]);
    assert_eq!(restored.settings, settings);
    assert_eq!(restored.messages, summary.messages);
    let core = lc_ffi::LichenCore::new(
        "t".into(),
        vec![2; 32],
        restored.identity_seed,
        Some(new_phone.path()),
    )
    .unwrap();
    assert_eq!(core.key(), sim.core.key());
    let c = find_contact(&core, "Cam").expect("the nickname came too");
    assert_eq!(c.identity, camille.identity);
    assert!(core.groups().iter().any(|g| g.gid == group.gid));
    assert_eq!(texts(&core.history(chat, None, 500).unwrap()), before);
}

/// ST-4: a problem report: the app's lines and the core's, without a name, a text or a key.
#[test]
fn st_4_problem_report_carries_no_secret() {
    let mut sim = Sim::new(164);
    let camille = sim.camille();
    sim.core
        .send_text(
            camille.key.clone(),
            "The spare key is under the blue stone".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    sim.core
        .rename_contact(camille.key.clone(), Some("Granny".into()))
        .unwrap();
    sim.run_for(60_000);
    let report = sim.core.problem_report(
        ReportApp {
            app_version: "0.10.3 (77)".into(),
            platform: "Android 16 (API 36)".into(),
            device: "Google Pixel 9".into(),
            link: "demo".into(),
            settings: vec!["Tor: off".into()],
            log: vec![ReportLine {
                at_ms: 1_000,
                text: "Opened Camille's chat".into(),
            }],
            private_words: vec![],
        },
        sim.now,
    );
    assert!(report.contains("lichen problem report"));
    assert!(report.contains("0.10.3 (77)"));
    assert!(report.contains("Setting:   Tor: off"));
    let lower = report.to_lowercase();
    for secret in [
        "camille",
        "granny",
        "spare key",
        "hi! i'm at the refuge",
        &hex(&camille.key),
        &hex(&camille.key[..4]),
        &hex(&sim.core.key().unwrap()),
        &hex(&sim.core.node_key().unwrap()[..4]),
    ] {
        assert!(
            !lower.contains(secret),
            "the report shows {secret:?}:\n{report}"
        );
    }
}

/// ST-8: the bridge as Settings starts it: no transport until asked (no endpoint, no onion
/// service), content offered and revoked, and the onion address of a key.
#[test]
fn st_8_bridge_settings() {
    let dir = TempDir::new("st8");
    let bridge = LichenBridge::new(vec![168; 32], Some(dir.path())).unwrap();
    assert!(LichenBridge::new(vec![1; 31], None).is_err());
    let rt = tokio::runtime::Runtime::new().unwrap();
    assert_eq!(rt.block_on(bridge.onion_status()), OnionStatus::Off);
    assert_eq!(rt.block_on(bridge.onion_id()), None);
    assert_eq!(bridge.endpoint_id(), None, "iroh off until started");
    assert!(!bridge.has_offers());
    let photo = vec![7u8; 50_000];
    let o = bridge.offer(photo).unwrap();
    assert_eq!((o.hash.len(), o.size), (32, 50_000));
    assert!(bridge.has_offers());
    bridge.revoke(o.hash.clone()).unwrap();
    assert!(!bridge.has_offers());
    assert!(bridge.revoke(vec![1; 3]).is_err());
    // Offers are kept in the folder: a bridge started again still has them.
    let o = bridge.offer(vec![8u8; 1_000]).unwrap();
    drop(bridge);
    let again = LichenBridge::new(vec![168; 32], Some(dir.path())).unwrap();
    assert!(again.has_offers());
    again.revoke(o.hash).unwrap();
    let addr = onion_address(vec![9; 32]).unwrap();
    assert!(addr.ends_with(".onion") && addr.len() == 62, "{addr}");
    assert_eq!(onion_address(vec![9; 31]), None);
}
