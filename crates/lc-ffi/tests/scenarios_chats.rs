//! Direct lichen chats (docs/testing.md §4, DM-1 to DM-14), with Camille and Julien on the
//! demo network.

mod common;

use common::{Sim, contact_chat, copies, find_contact, text, texts};
use lc_ffi::{
    ChatRef, LichenError, LichenEvent, Message, MessageStatus, QuoteKind, StoredMessage,
    TextTarget, text_links,
};

/// Camille's hello at 30 s, as stored.
fn hello(sim: &mut Sim) -> StoredMessage {
    let camille = sim.contact("Camille");
    let chat = contact_chat(&camille);
    sim.until_some("Camille's hello", 90, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| text(&m.message).is_some_and(|t| t.starts_with("Hi! I'm at the refuge")))
    })
}

/// Sends `s` to Camille and waits until it is delivered.
fn send_to_camille(sim: &mut Sim, s: &str) -> StoredMessage {
    let camille = sim.camille();
    let m = sim
        .core
        .send_text(camille.key.clone(), s.into(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&contact_chat(&camille), m.id, 600, |s| {
        *s == MessageStatus::Delivered
    })
}

fn echo(s: &str) -> String {
    format!("Copy: \"{s}\". I'm at the refuge, one hop away.")
}

/// DM-1: a text is accepted at once, then delivered, and shown once (Camille answers once).
#[test]
fn dm_1_send_a_text() {
    let mut sim = Sim::new(101);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let m = sim
        .core
        .send_text(
            camille.key.clone(),
            "On my way".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    assert!(m.outgoing && m.read);
    assert!(
        matches!(
            m.status,
            MessageStatus::Waiting { .. } | MessageStatus::Sending { .. }
        ),
        "{:?}",
        m.status
    );
    assert_eq!(text(&m.message), Some("On my way"));
    sim.until_status(&chat, m.id, 120, |s| *s == MessageStatus::Delivered);
    sim.until("Camille's answer", 120, |c| {
        copies(
            &c.history(chat.clone(), None, 50).unwrap(),
            &echo("On my way"),
        ) > 0
    });
    sim.run_for(30_000);
    let h = sim.history(&chat);
    assert_eq!(copies(&h, "On my way"), 1);
    assert_eq!(copies(&h, &echo("On my way")), 1);
    assert_eq!(h.iter().filter(|x| x.id == m.id).count(), 1);
    // Her answer is dated after our text, on the app's clock (B-core-3).
    let answer = h
        .iter()
        .find(|x| text(&x.message) == Some(echo("On my way").as_str()))
        .unwrap();
    let ours = h.iter().find(|x| x.id == m.id).unwrap();
    assert!(
        answer.at_ms > ours.at_ms,
        "{} <= {}",
        answer.at_ms,
        ours.at_ms
    );
    assert!(answer.at_ms <= sim.unix_ms());
    // The row in Messages shows the latest.
    let row = sim.conversation(&chat).unwrap();
    assert_eq!(text(&row.last.message), Some(echo("On my way").as_str()));
    // Nothing to send, nobody to send it to.
    assert_eq!(
        sim.core
            .send_text(camille.key.clone(), "".into(), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::Empty)
    );
    assert_eq!(
        sim.core
            .send_text(vec![0x44; 32], "x".into(), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::UnknownContact)
    );
}

/// DM-2: Camille's hello arrives: the event for the notification, the row in Messages with
/// her name, the text and one unread.
#[test]
fn dm_2_receive_a_text() {
    let mut sim = Sim::new(102);
    let camille = sim.contact("Camille");
    let (from, message) = sim.until_event("Camille's hello", 90, |e| match e {
        LichenEvent::Received { from, message, .. } => Some((from.clone(), message.clone())),
        _ => None,
    });
    assert_eq!(from, camille.key);
    assert!(text(&message).is_some_and(|t| t.starts_with("Hi! I'm at the refuge")));
    let chat = contact_chat(&camille);
    let row = sim.conversation(&chat).expect("a row in Messages");
    assert_eq!(row.name, "Camille");
    assert_eq!(row.unread, 1);
    assert!(!row.last.outgoing && !row.last.read);
    assert_eq!(row.last.status, MessageStatus::Received);
    // The history change came with it.
    assert!(sim.events.iter().any(|(_, e)| matches!(e,
        LichenEvent::MessageChanged { message } if message.id == row.last.id)));
}

/// DM-3: a long text goes in fragments and arrives whole; Camille's answer quoting it all
/// arrives whole too.
#[test]
fn dm_3_long_text() {
    let mut sim = Sim::new(103);
    let long: String = (0..60)
        .map(|i| format!("Line {i}: the path above the refuge is icy, take crampons. "))
        .collect();
    assert!(long.len() > 3_000);
    let camille = sim.camille();
    let est = sim
        .core
        .estimate_text(camille.key.clone(), long.len() as u32)
        .unwrap();
    assert!(est.fragments > 1, "{est:?}");
    let m = send_to_camille(&mut sim, &long);
    assert_eq!(m.fragments, est.fragments);
    assert_eq!(text(&m.message), Some(long.as_str()));
    let chat = contact_chat(&camille);
    sim.until("her answer, whole", 600, |c| {
        copies(&c.history(chat.clone(), None, 50).unwrap(), &echo(&long)) == 1
    });
}

/// DM-4: a reply quotes Camille's hello; the quote leads to it, and loads it in place when it
/// is older than what is shown.
#[test]
fn dm_4_reply_quotes_the_message() {
    let mut sim = Sim::new(104);
    let hello = hello(&mut sim);
    // Marc's MeshCore message (45 s), before we send anything that could collide with it.
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh { key: marc.key };
    let plain = sim.until_some("Marc's message", 240, |c| {
        c.history(mesh.clone(), None, 10)
            .unwrap()
            .into_iter()
            .next()
    });
    let chat = hello.chat.clone();
    assert!(hello.referable);
    let r = sim
        .core
        .send_reply(
            chat.clone(),
            hello.id,
            "Great, see you there".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let q = r.quote.clone().expect("a quote");
    assert_eq!(q.message_id, Some(hello.id));
    assert!(!q.outgoing);
    assert_eq!(q.kind, QuoteKind::Text);
    assert!(q.excerpt.starts_with("Hi! I'm at the refuge"), "{q:?}");
    let r = sim.until_status(&chat, r.id, 120, |s| *s == MessageStatus::Delivered);
    assert_eq!(r.quote.unwrap().message_id, Some(hello.id));
    // Tapped: the conversation from the quoted message on.
    let shown = sim.core.history_from(chat.clone(), hello.id, r.id).unwrap();
    assert_eq!(shown.first().map(|m| m.id), Some(hello.id));
    assert!(shown.iter().all(|m| m.id != r.id));
    // A reply to our own message quotes us.
    let ours = sim
        .core
        .send_reply(
            chat.clone(),
            r.id,
            "And bring the map".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let q2 = ours.quote.clone().unwrap();
    assert!(q2.outgoing);
    assert_eq!(q2.message_id, Some(r.id));
    assert_eq!(q2.excerpt, "Great, see you there");
    // Her hello deleted here: the reply's quote leads nowhere ("A message not on this
    // phone").
    sim.core.delete_for_me(hello.id).unwrap();
    let q = sim.message(&chat, r.id).quote.unwrap();
    assert_eq!(q.message_id, None, "{q:?}");
    // A MeshCore message has no reference: it cannot be answered this way.
    assert!(!plain.referable);
    assert!(
        sim.core
            .send_reaction(plain.id, "👍".into(), sim.now)
            .is_err(),
        "no reactions in a MeshCore chat"
    );
    assert_eq!(
        sim.core
            .send_reply(mesh, plain.id, "x".into(), sim.unix_s(), sim.now)
            .err(),
        Some(LichenError::NotReferable)
    );
}

/// DM-5: our reaction to her hello, changed, removed; hers to our text shows on it.
#[test]
fn dm_5_react_change_and_remove() {
    let mut sim = Sim::new(105);
    let hello = hello(&mut sim);
    let chat = hello.chat.clone();
    let m = sim
        .core
        .send_reaction(hello.id, "👍".into(), sim.now)
        .unwrap();
    assert_eq!(m.reactions.len(), 1);
    assert_eq!(
        (
            m.reactions[0].emoji.as_str(),
            m.reactions[0].count,
            m.reactions[0].mine
        ),
        ("👍", 1, true)
    );
    sim.run_for(10_000);
    let m = sim
        .core
        .send_reaction(hello.id, "❤️".into(), sim.now)
        .unwrap();
    assert_eq!(m.reactions.len(), 1, "{:?}", m.reactions);
    assert_eq!(m.reactions[0].emoji, "❤️");
    sim.run_for(10_000);
    let m = sim
        .core
        .send_reaction(hello.id, "".into(), sim.now)
        .unwrap();
    assert!(m.reactions.is_empty(), "{:?}", m.reactions);
    assert!(sim.message(&chat, hello.id).reactions.is_empty());
    // Too long for a reaction.
    assert!(
        sim.core
            .send_reaction(hello.id, "👍".repeat(20), sim.now)
            .is_err()
    );
    // Camille gives our text a 👍 (8 s after her answer).
    let ours = send_to_camille(&mut sim, "Leaving now");
    let (by, emoji, target) = sim.until_event("Camille's reaction", 180, |e| match e {
        LichenEvent::Reacted {
            by, emoji, target, ..
        } => Some((by.clone(), emoji.clone(), target.clone())),
        _ => None,
    });
    assert_eq!(by, find_contact(&sim.core, "Camille").unwrap().key);
    assert_eq!(emoji, "👍");
    assert_eq!(target.map(|t| t.id), Some(ours.id));
    let shown = sim.message(&chat, ours.id);
    assert_eq!(shown.reactions.len(), 1);
    assert!(!shown.reactions[0].mine);
    // Nothing failed on the way.
    assert!(
        !sim.events
            .iter()
            .any(|(_, e)| matches!(e, LichenEvent::Failed { .. }))
    );
}

/// DM-6: our text edited: the new text and the mark here; hers cannot be edited by us.
#[test]
fn dm_6_edit_a_sent_message() {
    let mut sim = Sim::new(106);
    let ours = send_to_camille(&mut sim, "See you at 6");
    assert!(ours.editable && !ours.edited);
    let chat = ours.chat.clone();
    let e = sim
        .core
        .send_edit(ours.id, "See you at 7".into(), sim.now)
        .unwrap();
    assert!(e.edited);
    assert_eq!(text(&e.message), Some("See you at 7"));
    sim.run_for(30_000);
    let shown = sim.message(&chat, ours.id);
    assert!(shown.edited);
    assert_eq!(text(&shown.message), Some("See you at 7"));
    assert_eq!(shown.status, MessageStatus::Delivered);
    assert_eq!(copies(&sim.history(&chat), "See you at 6"), 0);
    // Hers: not ours to change.
    let hello = hello(&mut sim);
    assert!(!hello.editable && !hello.deletable);
    assert!(sim.core.send_edit(hello.id, "x".into(), sim.now).is_err());
    assert!(
        sim.core.send_edit(ours.id, "".into(), sim.now).is_err(),
        "an empty edit"
    );
    assert!(
        !sim.events
            .iter()
            .any(|(_, e)| matches!(e, LichenEvent::Failed { .. }))
    );
}

/// DM-7: delete for me removes her hello here only; delete for everyone leaves "message
/// deleted" in its place, and it can no longer be edited or deleted.
#[test]
fn dm_7_delete_for_me_and_for_everyone() {
    let mut sim = Sim::new(107);
    let hello = hello(&mut sim);
    let chat = hello.chat.clone();
    sim.core.delete_for_me(hello.id).unwrap();
    assert!(sim.history(&chat).iter().all(|m| m.id != hello.id));
    assert!(
        sim.core
            .search_messages("refuge, the mobile".into(), 10)
            .is_empty()
    );
    let ours = send_to_camille(&mut sim, "Wrong chat, sorry");
    assert!(ours.deletable);
    let d = sim.core.send_delete(ours.id, sim.now).unwrap();
    assert!(matches!(d.message, Message::Deleted { .. }), "{d:?}");
    assert!(!d.editable && !d.deletable);
    sim.run_for(30_000);
    let shown = sim.message(&chat, ours.id);
    assert!(matches!(shown.message, Message::Deleted { .. }));
    assert!(sim.core.send_edit(ours.id, "x".into(), sim.now).is_err());
    assert!(
        sim.core.search_messages("Wrong chat".into(), 10).is_empty(),
        "its text is gone"
    );
}

/// DM-8: a 5-minute timer (`-timer 300`): noted in the chat, our text and her answer vanish
/// after it; turned off, messages stay.
#[test]
fn dm_8_disappearing_timer() {
    let mut sim = Sim::new(108);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    sim.core.set_disappearing(chat.clone(), 300).unwrap();
    assert_eq!(sim.core.disappearing(chat.clone()).unwrap(), 300);
    let notice = sim
        .history(&chat)
        .into_iter()
        .rev()
        .find_map(|m| match m.message {
            Message::Notice { kind: 11, text, .. } => Some(text),
            _ => None,
        });
    assert_eq!(notice.as_deref(), Some("300"));
    let ours = send_to_camille(&mut sim, "This will vanish");
    sim.until("her answer", 120, |c| {
        copies(
            &c.history(chat.clone(), None, 50).unwrap(),
            &echo("This will vanish"),
        ) == 1
    });
    // Still there after 4 minutes.
    sim.run_for(240_000);
    assert_eq!(copies(&sim.history(&chat), "This will vanish"), 1);
    sim.until("both gone", 90, |c| {
        let h = c.history(chat.clone(), None, 50).unwrap();
        copies(&h, "This will vanish") == 0 && copies(&h, &echo("This will vanish")) == 0
    });
    assert!(sim.events.iter().any(|(_, e)| matches!(e,
        LichenEvent::MessageRemoved { id, .. } if *id == ours.id)));
    // Off: what comes now stays.
    sim.core.set_disappearing(chat.clone(), 0).unwrap();
    assert_eq!(sim.core.disappearing(chat.clone()).unwrap(), 0);
    let kept = send_to_camille(&mut sim, "This stays");
    sim.run_for(400_000);
    assert!(sim.history(&chat).iter().any(|m| m.id == kept.id));
    assert!(
        sim.core.set_disappearing(chat, 29 * 86_400).is_err(),
        "four weeks at most"
    );
}

/// DM-9: 250 older messages (`-fill 250`): pages of 50 back to the oldest, in order, without
/// duplicates; the last five of hers unread.
#[test]
fn dm_9_paging_older_messages() {
    let mut sim = Sim::new(109);
    let camille = sim.contact("Camille");
    let chat = contact_chat(&camille);
    sim.core.demo_fill_history(chat.clone(), 250).unwrap();
    hello(&mut sim);
    let mut seen: Vec<StoredMessage> = Vec::new();
    let mut before = None;
    loop {
        let page = sim.core.history(chat.clone(), before, 50).unwrap();
        if page.is_empty() {
            break;
        }
        assert!(page.len() <= 50);
        before = Some(page[0].id);
        let mut all = page;
        all.append(&mut seen);
        seen = all;
    }
    assert!(seen.len() >= 251, "{}", seen.len());
    let ids: std::collections::HashSet<i64> = seen.iter().map(|m| m.id).collect();
    assert_eq!(ids.len(), seen.len(), "no duplicates");
    assert!(
        seen.windows(2).all(|w| w[0].at_ms <= w[1].at_ms),
        "oldest first"
    );
    assert_eq!(text(&seen[0].message), Some("Earlier message 1"));
    assert!(
        text(&seen.last().unwrap().message).is_some_and(|t| t.starts_with("Hi! I'm at")),
        "the newest last"
    );
    // The unread: five filled ones and her hello.
    assert_eq!(sim.conversation(&chat).unwrap().unread, 6);
    // Out of the demo's in-memory store, filling is refused.
    let dir = common::TempDir::new("dm9");
    let persistent = common::new_core(9, 9, Some(&dir));
    assert_eq!(
        persistent.demo_fill_history(chat, 10),
        Err(LichenError::NotAllowed)
    );
}

/// DM-10: a search finds texts across chats (case and accents aside), newest first; opening
/// a result shows it in its conversation.
#[test]
fn dm_10_search_messages() {
    let mut sim = Sim::new(110);
    let hello = hello(&mut sim);
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh { key: marc.key };
    sim.until("Marc's message", 240, |c| {
        !c.history(mesh.clone(), None, 10).unwrap().is_empty()
    });
    let found = sim.core.search_messages("RÉFUGE".into(), 50);
    let chats: Vec<&ChatRef> = found.iter().map(|m| &m.chat).collect();
    assert!(chats.contains(&&hello.chat), "{found:?}");
    assert!(chats.contains(&&mesh), "{found:?}");
    assert!(
        found.windows(2).all(|w| w[0].at_ms >= w[1].at_ms),
        "newest first"
    );
    // Opening Camille's: her conversation, from that message on.
    let r = found.iter().find(|m| m.chat == hello.chat).unwrap();
    let newest = sim.history(&hello.chat).last().unwrap().id;
    let shown = sim
        .core
        .history_from(hello.chat.clone(), r.id, newest + 1)
        .unwrap();
    assert_eq!(shown.first().map(|m| m.id), Some(r.id));
    assert_eq!(shown.last().map(|m| m.id), Some(newest));
    // Too short a query finds nothing; one that matches nothing, nothing.
    assert!(sim.core.search_messages("r".into(), 50).is_empty());
    assert!(sim.core.search_messages("zebra".into(), 50).is_empty());
    assert_eq!(sim.core.search_messages("refuge".into(), 1).len(), 1);
}

/// DM-11: what Info shows: when, the route's hops, fragments, airtime; the signal of hers.
#[test]
fn dm_11_message_info() {
    let mut sim = Sim::new(111);
    let sent_at = sim.unix_ms();
    let ours = send_to_camille(&mut sim, "Info please");
    assert!((ours.at_ms - sent_at).abs() < 120_000, "{ours:?}");
    assert!(ours.fragments >= 1);
    assert!(ours.airtime_ms > 0);
    assert_eq!(ours.status, MessageStatus::Delivered);
    let camille = find_contact(&sim.core, "Camille").unwrap();
    assert_eq!(camille.hops, Some(1));
    let path = sim.core.node_path(camille.key.clone()).unwrap().unwrap();
    assert_eq!(path[0].names, vec!["hub".to_owned()]);
    let theirs = hello(&mut sim);
    assert!(theirs.snr_db.is_some());
    assert!(theirs.at_ms > 0 && theirs.at_ms <= sim.unix_ms());
}

/// DM-12: unread per conversation and in all; opening a conversation clears its own only.
#[test]
fn dm_12_unread_counts_and_mark_read() {
    let mut sim = Sim::new(112);
    let hello = hello(&mut sim);
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh { key: marc.key };
    sim.until("Marc's message", 240, |c| {
        c.conversations().iter().any(|v| v.chat == mesh)
    });
    let unread = |sim: &Sim, chat: &ChatRef| sim.conversation(chat).map_or(0, |c| c.unread);
    assert_eq!(unread(&sim, &hello.chat), 1);
    assert_eq!(unread(&sim, &mesh), 1);
    let total: u32 = sim.core.conversations().iter().map(|c| c.unread).sum();
    assert!(total >= 2, "the tab's badge: {total}");
    sim.core.mark_read(hello.chat.clone()).unwrap();
    assert_eq!(unread(&sim, &hello.chat), 0);
    assert!(sim.message(&hello.chat, hello.id).read);
    assert_eq!(unread(&sim, &mesh), 1, "the other stays unread");
    // Our own messages never count.
    send_to_camille(&mut sim, "Read you");
    sim.core.mark_read(hello.chat.clone()).unwrap();
    sim.run_for(1_000);
    assert_eq!(unread(&sim, &hello.chat), 0);
    // Her answer is unread again.
    sim.until("her answer", 120, |c| {
        c.conversations()
            .iter()
            .any(|v| v.chat == hello.chat && v.unread == 1)
    });
}

/// DM-13: Camille's hello forwarded to Julien: a new message of ours there, delivered, and
/// Julien answers it.
#[test]
fn dm_13_forward_a_message() {
    let mut sim = Sim::new(113);
    let hello = hello(&mut sim);
    let hello_text = text(&hello.message).unwrap().to_owned();
    let julien = sim.contact("Julien");
    let to = contact_chat(&julien);
    // Once the hub is quiet: in the demo's first minutes, the group's setup and Camille's
    // photo keep it busy.
    sim.run_to(330_000);
    let f = sim
        .core
        .forward(hello.id, to.clone(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(f.outgoing);
    assert_eq!(f.chat, to);
    assert_eq!(text(&f.message), Some(hello_text.as_str()));
    sim.until_status(&to, f.id, 240, |s| *s == MessageStatus::Delivered);
    sim.until("Julien's answer", 60, |c| {
        texts(&c.history(to.clone(), None, 50).unwrap())
            .iter()
            .any(|t| t.starts_with("Got it, two hops away") && t.contains(&hello_text))
    });
    // The original stays where it was.
    assert!(sim.history(&hello.chat).iter().any(|m| m.id == hello.id));
    // Into the group: a post of ours.
    let group = sim.rando();
    let gchat = ChatRef::Group { gid: group.gid };
    let g = sim
        .core
        .forward(hello.id, gchat.clone(), sim.unix_s(), sim.now)
        .unwrap();
    sim.until_status(&gchat, g.id, 120, |s| *s == MessageStatus::Flooded);
    assert_eq!(
        text(&sim.message(&gchat, g.id).message),
        Some(hello_text.as_str())
    );
    // Into Marc's MeshCore chat: a plain text, which he gets.
    let marc = sim.contact("Marc");
    let mesh = ChatRef::Mesh { key: marc.key };
    let p = sim
        .core
        .forward(hello.id, mesh.clone(), sim.unix_s(), sim.now)
        .unwrap();
    assert_eq!(text(&p.message), Some(hello_text.as_str()));
    let p = sim.until_status(&mesh, p.id, 120, |s| {
        matches!(s, MessageStatus::Delivered | MessageStatus::Failed { .. })
    });
    // Retried once as the app offers, as MC-1 does: the radio's three tries go out while the
    // group post just forwarded is still repeated (the hub and crest busy), so whether one of
    // them is confirmed depends on collisions any change to the demo's air reshuffles.
    if let MessageStatus::Failed { .. } = p.status {
        sim.core.retry_message(p.id, sim.now).unwrap();
        sim.until_status(&mesh, p.id, 120, |s| *s == MessageStatus::Delivered);
    }
}

/// DM-14: links and places in a text, as the screen marks them (UTF-16 offsets, after an
/// emoji); they come back in Camille's answer.
#[test]
fn dm_14_links_and_places_in_a_text() {
    let s = "🏔 Meet at 45.1972, 5.7245 then https://example.org/map ok";
    let links = text_links(s.into());
    assert_eq!(links.len(), 2, "{links:?}");
    let utf16: Vec<u16> = s.encode_utf16().collect();
    let span = |l: &lc_ffi::TextLink| {
        String::from_utf16(&utf16[l.start as usize..l.end as usize]).unwrap()
    };
    assert!(matches!(links[0].target, TextTarget::Place { lat, lon }
        if (lat - 45.1972).abs() < 1e-9 && (lon - 5.7245).abs() < 1e-9));
    assert_eq!(span(&links[0]), "45.1972, 5.7245");
    assert!(
        matches!(&links[1].target, TextTarget::Web { url } if url == "https://example.org/map")
    );
    assert_eq!(span(&links[1]), "https://example.org/map");
    let mut sim = Sim::new(114);
    send_to_camille(&mut sim, s);
    let camille = find_contact(&sim.core, "Camille").unwrap();
    let chat = contact_chat(&camille);
    let answer = sim.until_some("her answer", 120, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| !m.outgoing && text(&m.message) == Some(echo(s).as_str()))
    });
    let back = text_links(text(&answer.message).unwrap().into());
    assert_eq!(back.len(), 2);
    assert!(text_links("nothing here".into()).is_empty());
}
