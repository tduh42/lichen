//! Groups (docs/testing.md §7, GR-1 to GR-8): "Rando Vercors", Camille's, with Julien; groups of
//! our own; Léa, who scans an invitation.

mod common;

use common::{Sim, text, texts};
use lc_ffi::{
    ChatRef, LichenError, LichenEvent, Message, MessageStatus, decode_voice, group_voice_max_ms,
    prepare_voice, voice_datagram_len,
};

fn chat(gid: &[u8]) -> ChatRef {
    ChatRef::Group { gid: gid.to_vec() }
}

/// The notices of a group's history: kind and text.
fn notices(sim: &Sim, gid: &[u8]) -> Vec<(u8, String)> {
    sim.history(&chat(gid))
        .into_iter()
        .filter_map(|m| match m.message {
            Message::Notice { kind, text, .. } => Some((kind, text)),
            _ => None,
        })
        .collect()
}

/// GR-1: a group of our own, alone in it: in Messages with its "created" line; the groups
/// sorted as people read names.
#[test]
fn gr_1_create_a_group() {
    let mut sim = Sim::new(141);
    sim.until("connected", 10, |c| c.node_key().is_some());
    let gid = sim
        .core
        .create_group("Sortie du dimanche".into(), vec![], sim.now)
        .unwrap();
    sim.drain();
    let g = sim
        .core
        .groups()
        .into_iter()
        .find(|g| g.gid == gid)
        .unwrap();
    assert_eq!(g.name, "Sortie du dimanche");
    assert_eq!(Some(g.admin.clone()), sim.core.node_key(), "ours");
    assert!(
        g.members
            .iter()
            .all(|m| Some(&m.key) == sim.core.node_key().as_ref()),
        "alone: {:?}",
        g.members
    );
    assert_eq!(notices(&sim, &gid), [(1, String::new())]);
    let row = sim.conversation(&chat(&gid)).expect("in Messages");
    assert_eq!(row.name, "Sortie du dimanche");
    assert_eq!(row.unread, 0, "our own line is read");
    // A post there floods to nobody, and is kept.
    let m = sim
        .core
        .send_group_text(gid.clone(), "Anyone?".into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&chat(&gid), m.id, 30, |s| *s == MessageStatus::Flooded);
    // Names: blank refused; a long one cut as the group's state carries it to the members
    // (32 bytes), as a rename is.
    assert_eq!(
        sim.core.create_group("  ".into(), vec![], sim.now).err(),
        Some(LichenError::Empty)
    );
    let long = sim
        .core
        .create_group(
            "  Traversée du Vercors en trois jours ".into(),
            vec![],
            sim.now,
        )
        .unwrap();
    let name = |sim: &Sim, gid: &[u8]| {
        sim.core
            .groups()
            .into_iter()
            .find(|g| g.gid == gid)
            .unwrap()
            .name
    };
    assert_eq!(name(&sim, &long), "Traversée du Vercors en trois j");
    // The list, sorted as people read names.
    for n in ["Zermatt", "école de ski", "alpinistes"] {
        sim.core.create_group(n.into(), vec![], sim.now).unwrap();
    }
    let names: Vec<String> = sim.core.groups().into_iter().map(|g| g.name).collect();
    assert_eq!(
        names,
        [
            "alpinistes",
            "école de ski",
            "Sortie du dimanche",
            "Traversée du Vercors en trois j",
            "Zermatt"
        ]
    );
}

/// GR-2: our group's invitation scanned by Léa beside us: she joins, we see it, and her hello
/// arrives under her name.
#[test]
fn gr_2_invite_by_qr_someone_joins() {
    let mut sim = Sim::new(142);
    sim.until("connected", 10, |c| c.node_key().is_some());
    let gid = sim
        .core
        .create_group("Sortie du dimanche".into(), vec![], sim.now)
        .unwrap();
    let code = sim.core.create_invite(gid.clone(), sim.now).unwrap();
    assert!(code.expires > u64::from(sim.unix_s()));
    assert_eq!(
        sim.core.parse_invite(code.text.clone(), sim.now),
        Err(LichenError::BadCode),
        "our own"
    );
    assert!(
        sim.core.own_code(code.text.clone()),
        "and the app can say so"
    );
    sim.demo.guest_scans(code.text.clone());
    let node = sim.until_event("Léa admitted", 300, |e| match e {
        LichenEvent::MemberJoined { gid: g, node } if *g == gid => Some(node.clone()),
        _ => None,
    });
    let g = sim
        .core
        .groups()
        .into_iter()
        .find(|g| g.gid == gid)
        .unwrap();
    let lea = g.members.iter().find(|m| m.key == node).expect("listed");
    assert_eq!(lea.name, "Léa");
    let hello = sim.until_some("her hello", 300, |c| {
        c.history(chat(&gid), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| text(&m.message).is_some_and(|t| t.starts_with("Hi, it's Léa")))
    });
    assert!(!hello.outgoing);
    assert_eq!(
        notices(&sim, &gid),
        [(1, String::new()), (3, String::new())],
        "created, Léa joined"
    );
    // Revoked: the code admits no one more.
    sim.core.revoke_invite(code.token);
}

/// GR-3: Camille's invitation scanned: what it says before joining; joined, the group opens with
/// its members, and the admin is a verified contact.
#[test]
fn gr_3_join_by_scanning_an_invitation() {
    let mut sim = Sim::new(143);
    sim.until("connected", 10, |c| c.node_key().is_some());
    let demo = sim.demo.clone();
    let code = sim.until_some("Camille's invitation", 60, |_| demo.camille_invite());
    let info = sim.core.parse_invite(code.clone(), sim.now).unwrap();
    assert_eq!(
        (info.group_name.as_str(), info.admin_name.as_str()),
        ("Refuge tonight", "Camille")
    );
    assert!(!info.expired && !info.member);
    let admin = sim.core.accept_invite(code.clone(), true, sim.now).unwrap();
    assert_eq!(admin.key, info.admin);
    assert!(admin.verified, "scanned in person");
    let gid = sim.until_event("admitted", 300, |e| match e {
        LichenEvent::GroupJoined { gid, name, admin } if name == "Refuge tonight" => {
            assert_eq!(*admin, info.admin);
            Some(gid.clone())
        }
        _ => None,
    });
    let g = sim
        .core
        .groups()
        .into_iter()
        .find(|g| g.gid == gid)
        .unwrap();
    assert!(g.members.iter().any(|m| m.key == info.admin));
    assert!(
        sim.conversation(&chat(&gid)).is_some(),
        "it opens in Messages"
    );
    // Scanned again: already a member.
    assert!(sim.core.parse_invite(code, sim.now).unwrap().member);
    assert_eq!(
        sim.core.parse_invite("lichen:9:nope".into(), sim.now),
        Err(LichenError::BadCode)
    );
}

/// GR-4: Camille's group with consent on: an invitation card in Messages; accepted, we join and
/// her posts come; declined, it goes and nothing of it comes.
#[test]
fn gr_4_invitation_received_accept_and_decline() {
    for accept in [true, false] {
        let mut sim = Sim::new(144 + u8::from(accept));
        // The app asks before joining a group (the default outside the demo).
        sim.core.set_group_consent(true);
        let (gid, name, admin, members) = sim.until_event("the invitation", 300, |e| match e {
            LichenEvent::GroupInvited {
                gid,
                name,
                admin,
                members,
            } => Some((gid.clone(), name.clone(), admin.clone(), *members)),
            _ => None,
        });
        assert_eq!(name, "Rando Vercors");
        assert_eq!(members, 3);
        let camille = common::find_contact(&sim.core, "Camille").unwrap();
        assert_eq!(admin, camille.key);
        let inv = sim.core.group_invitations();
        assert_eq!(inv.len(), 1);
        assert_eq!(
            (inv[0].name.as_str(), inv[0].admin_name.as_str()),
            ("Rando Vercors", "Camille")
        );
        assert!(inv[0].members.iter().any(|m| m == "Julien"), "{inv:?}");
        assert!(sim.core.groups().is_empty(), "nothing joined yet");
        if accept {
            sim.core.accept_group(gid.clone(), sim.now).unwrap();
            sim.until_event("joined", 30, |e| match e {
                LichenEvent::GroupJoined { gid: g, .. } if *g == gid => Some(()),
                _ => None,
            });
            assert!(sim.core.group_invitations().is_empty());
            sim.until("Julien's first post", 900, |c| {
                texts(&c.history(chat(&gid), None, 50).unwrap())
                    .iter()
                    .any(|t| t.starts_with("Leaving the col"))
            });
        } else {
            sim.core.decline_group(gid.clone()).unwrap();
            assert!(sim.core.group_invitations().is_empty());
            sim.run_to(600_000);
            assert!(sim.core.groups().is_empty());
            assert!(sim.history(&chat(&gid)).is_empty(), "nothing of it");
            assert!(sim.conversation(&chat(&gid)).is_none());
        }
    }
}

/// GR-5: a post of ours floods; Camille answers it; Julien's posts come under his name, in
/// order.
#[test]
fn gr_5_post_and_receive_in_a_group() {
    let mut sim = Sim::new(146);
    let g = sim.rando();
    let gchat = chat(&g.gid);
    // Once the hub is quiet: a flood in the demo's first minutes may be lost, and nobody
    // acknowledges one.
    sim.run_to(330_000);
    let m = sim
        .core
        .send_group_text(g.gid.clone(), "On my way up".into(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(matches!(
        m.status,
        MessageStatus::Waiting { .. } | MessageStatus::Sending { .. }
    ));
    sim.until_status(&gchat, m.id, 60, |s| *s == MessageStatus::Flooded);
    let camille = common::find_contact(&sim.core, "Camille").unwrap();
    let julien = common::find_contact(&sim.core, "Julien").unwrap();
    let seen = sim.until_some("Camille's answer", 300, |c| {
        c.history(gchat.clone(), None, 100)
            .unwrap()
            .into_iter()
            .find(|m| text(&m.message).is_some_and(|t| t.starts_with("Seen.")))
    });
    assert_eq!(
        seen.author,
        Some(camille.key.clone()),
        "her node as the author"
    );
    let leaving = sim.until_some("Julien's post", 900, |c| {
        c.history(gchat.clone(), None, 100)
            .unwrap()
            .into_iter()
            .find(|m| text(&m.message).is_some_and(|t| t.starts_with("Leaving the col")))
    });
    assert_eq!(
        leaving.author,
        Some(julien.key.clone()),
        "his node as the author"
    );
    let h = sim.history(&gchat);
    assert!(h.windows(2).all(|w| w[0].at_ms <= w[1].at_ms), "in order");
    assert_eq!(common::copies(&h, "On my way up"), 1);
    // Unknown group, empty text.
    assert!(
        sim.core
            .send_group_text(vec![9; 16], "x".into(), sim.unix_s(), sim.now)
            .is_err()
    );
    assert!(
        sim.core
            .send_group_text(g.gid, "".into(), sim.unix_s(), sim.now)
            .is_err()
    );
}

/// GR-6: Julien's second post, missed on the air, comes back from a member after his GIF
/// revealed the gap: marked late, where it arrived, with its own time.
#[test]
fn gr_6_a_post_lost_recovered_late() {
    let mut sim = Sim::new(147);
    let g = sim.rando();
    let gchat = chat(&g.gid);
    let mist = sim.until_some("the mist post, late", 20 * 60, |c| {
        c.history(gchat.clone(), None, 100)
            .unwrap()
            .into_iter()
            .find(|m| text(&m.message).is_some_and(|t| t.starts_with("Mist on the ridge")))
    });
    assert!(mist.late, "{mist:?}");
    let h = sim.history(&gchat);
    let gif = h
        .iter()
        .position(|m| matches!(m.message, Message::Gif { .. }))
        .expect("his GIF");
    let at = h.iter().position(|m| m.id == mist.id).unwrap();
    assert!(gif < at, "shown where it arrived, after the GIF");
    let (
        Message::Text {
            sent_at: mist_sent, ..
        },
        Message::Gif {
            sent_at: gif_sent, ..
        },
    ) = (&mist.message, &h[gif].message)
    else {
        unreachable!()
    };
    assert!(mist_sent < gif_sent, "written before the GIF");
    assert_eq!(common::copies(&h, text(&mist.message).unwrap()), 1);
}

/// GR-7: a group voice note: 6 s at most in 700C (5 fragments), its cost known first; it
/// floods and plays; longer is refused.
#[test]
fn gr_7_group_voice_note() {
    let mut sim = Sim::new(148);
    let g = sim.rando();
    let max = group_voice_max_ms(0);
    assert!((6_000..=7_000).contains(&max), "{max}");
    let pcm: Vec<i16> = (0..(max as usize * 8))
        .map(|i| ((i % 80) as i16 - 40) * 200)
        .collect();
    let v = prepare_voice(pcm, 0);
    let cost = sim.core.estimate_group_datagram(voice_datagram_len(&v));
    assert!(cost.fragments <= 5 && cost.airtime_ms > 0, "{cost:?}");
    let m = sim
        .core
        .send_group_voice(g.gid.clone(), v, sim.unix_s(), sim.now)
        .unwrap();
    let m = sim.until_status(&chat(&g.gid), m.id, 120, |s| *s == MessageStatus::Flooded);
    let Message::Voice {
        mode, frames, data, ..
    } = m.message
    else {
        unreachable!()
    };
    assert_eq!(
        decode_voice(mode, frames, data).unwrap().len(),
        usize::from(frames) * 320
    );
    // Twelve seconds: too long for a group.
    let long = prepare_voice(vec![100; 12 * 8_000], 0);
    assert!(
        sim.core
            .send_group_voice(g.gid.clone(), long, sim.unix_s(), sim.now)
            .is_err()
    );
    // Nothing recorded: nothing goes.
    assert_eq!(
        sim.core
            .send_group_voice(g.gid, prepare_voice(vec![], 0), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::Empty)
    );
}

/// GR-8: our group with Camille: renamed (a line for it), a member added and removed; closed
/// when we leave. Camille's group left: nothing more of it arrives.
#[test]
fn gr_8_rename_members_and_leave() {
    let mut sim = Sim::new(149);
    let camille = sim.camille();
    let rando = sim.rando();
    let julien = common::find_contact(&sim.core, "Julien").unwrap();
    let gid = sim
        .core
        .create_group("Refuge crew".into(), vec![camille.key.clone()], sim.now)
        .unwrap();
    sim.core
        .rename_group(gid.clone(), "Refuge crew 2".into(), sim.now)
        .unwrap();
    sim.drain();
    assert!(notices(&sim, &gid).contains(&(9, "Refuge crew 2".into())));
    assert!(sim.core.groups().iter().any(|g| g.name == "Refuge crew 2"));
    sim.core
        .add_members(gid.clone(), vec![julien.key.clone()], sim.now)
        .unwrap();
    sim.until("Julien added", 60, |c| {
        c.groups()
            .iter()
            .any(|g| g.gid == gid && g.members.iter().any(|m| m.key == julien.key))
    });
    sim.core
        .remove_member(gid.clone(), julien.key.clone(), sim.now)
        .unwrap();
    sim.until("Julien removed", 60, |c| {
        c.groups()
            .iter()
            .any(|g| g.gid == gid && !g.members.iter().any(|m| m.key == julien.key))
    });
    // Not ours: Camille's group cannot be renamed by us.
    assert!(
        sim.core
            .rename_group(rando.gid.clone(), "Mine".into(), sim.now)
            .is_err()
    );
    // We leave ours: closed.
    sim.core.leave_group(gid.clone(), sim.now).unwrap();
    sim.until("ours left", 60, |c| {
        !c.groups().iter().any(|g| g.gid == gid)
    });
    // We leave Camille's before Julien posts: nothing of it comes any more.
    sim.core.leave_group(rando.gid.clone(), sim.now).unwrap();
    let from = sim.events.len();
    sim.until("hers left", 60, |c| {
        !c.groups().iter().any(|g| g.gid == rando.gid)
    });
    let kept = sim.history(&chat(&rando.gid)).len();
    sim.run_to(15 * 60_000);
    assert!(
        !sim.since(from).any(|e| matches!(e,
            LichenEvent::GroupMessage { gid, .. } if *gid == rando.gid)),
        "a post of a group we left"
    );
    assert!(sim.history(&chat(&rando.gid)).len() <= kept + 1);
}
