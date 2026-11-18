//! Unit tests of the binding layer.

use super::*;

/// A direct message as the core reports it, with its plaintext as sent.
#[cfg(test)]
fn received(from: [u8; 32], content: Content) -> Event {
    Event::Received {
        from,
        plaintext: content.encode(),
        content,
        snr4: 0,
        rssi: 0,
    }
}

/// A group message as the core reports it, with its plaintext as sent.
#[cfg(test)]
fn group_message(
    gid: [u8; 16],
    from: [u8; 32],
    author: [u8; 32],
    content: Content,
    late: bool,
) -> Event {
    Event::GroupMessage {
        gid,
        from,
        author,
        plaintext: content.encode(),
        content,
        snr4: 0,
        rssi: 0,
        late,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long conversation pages back to its first message (audit M06).
    #[test]
    fn history_pages_back_to_the_oldest() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![101; 32], None).unwrap();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        core.demo_fill_history(chat.clone(), 250).unwrap();
        let mut all: Vec<StoredMessage> = Vec::new();
        let mut before = None;
        loop {
            let page = core.history(chat.clone(), before, 100).unwrap();
            let n = page.len();
            before = page.first().map(|m| m.id);
            all.splice(0..0, page);
            if n < 100 {
                break;
            }
        }
        assert_eq!(all.len(), 250);
        assert!(all.windows(2).all(|w| w[0].id < w[1].id));
        assert!(
            matches!(&all[0].message, Message::Text { text, .. } if text == "Earlier message 1")
        );
        // A search result far back, in one read (recheck R08).
        let newest = core.history(chat.clone(), None, 100).unwrap();
        let gap = core
            .history_from(chat.clone(), all[3].id, newest[0].id)
            .unwrap();
        assert_eq!(gap.len(), 147);
        assert_eq!(gap[0].id, all[3].id);
        // On a real database, refused.
        let dir = std::env::temp_dir().join(format!("lichen-fill-{}", std::process::id()));
        let real = LichenCore::new(
            "t".into(),
            vec![2; 32],
            vec![102; 32],
            Some(dir.display().to_string()),
        )
        .unwrap();
        assert!(matches!(
            real.demo_fill_history(chat, 1),
            Err(LichenError::NotAllowed)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn constructs_and_connects() {
        let core = LichenCore::new("test".into(), vec![1; 32], vec![2; 32], None).unwrap();
        core.connect(0);
        let first = core.poll_companion(0).unwrap();
        assert_eq!(first[0], 1, "APP_START comes first");
        assert!(LichenCore::new("x".into(), vec![1; 31], vec![2; 32], None).is_err());
        assert!(LichenCore::new("x".into(), vec![1; 32], vec![2; 31], None).is_err());
        assert_eq!(
            core.send_text(vec![0; 3], "x".into(), 0, 0),
            Err(LichenError::InvalidKey)
        );
        assert_eq!(
            core.send_text(vec![0; 32], "x".into(), 0, 0),
            Err(LichenError::UnknownContact)
        );
    }

    #[test]
    fn usb_framing_round_trip() {
        let frame = vec![0x84, 1, 2, 3];
        let wire = usb_encode(frame.clone()).unwrap();
        assert_eq!(&wire[..3], &[b'<', 4, 0]);
        assert!(usb_encode(vec![]).is_err());
        assert!(usb_encode(vec![0; 177]).is_err());
        // The radio's direction, split across reads, with noise before the first frame.
        let d = UsbDecoder::new();
        assert!(d.push(vec![0xAA, b'>', 4]).is_empty());
        let mut rest = vec![0];
        rest.extend_from_slice(&frame);
        rest.extend_from_slice(&[b'>', 1, 0, 0x88]);
        assert_eq!(d.push(rest), vec![frame, vec![0x88]]);
    }

    #[test]
    fn qr_codes() {
        let q = qr_code("lichen:1:test".into()).unwrap();
        assert!(q.width >= 21);
        assert_eq!(q.dark.len(), (q.width * q.width) as usize);
        // The finder pattern's corner is dark.
        assert!(q.dark[0]);
    }
}

#[cfg(test)]
mod essentials {
    use super::*;

    /// Search across conversations, the unread flag, pins and mutes (chat essentials).
    #[test]
    fn search_unread_pins_and_mutes() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![121; 32], None).unwrap();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        core.demo_fill_history(chat.clone(), 30).unwrap();
        let found = core.search_messages("EARLIER message 12".into(), 50);
        assert_eq!(found.len(), 1);
        assert!(
            matches!(&found[0].message, Message::Text { text, .. } if text == "Earlier message 12")
        );
        assert_eq!(core.search_messages("earlier".into(), 50).len(), 30);
        assert_eq!(core.search_messages("earlier".into(), 7).len(), 7);
        assert!(core.search_messages("e".into(), 50).is_empty());

        let unread = |c: &LichenCore| {
            c.history(chat.clone(), None, 100)
                .unwrap()
                .iter()
                .filter(|m| !m.read)
                .count()
        };
        assert_eq!(unread(&core), 5);
        core.mark_read(chat.clone()).unwrap();
        assert_eq!(unread(&core), 0);

        core.set_pinned(chat.clone(), true).unwrap();
        core.set_muted(chat.clone(), Some(i64::MAX)).unwrap();
        let prefs = core.chat_prefs();
        assert_eq!(prefs.len(), 1);
        assert!(prefs[0].pinned_at_ms.is_some());
        assert_eq!(prefs[0].muted_until_ms, Some(i64::MAX));
        core.set_pinned(chat.clone(), false).unwrap();
        core.set_muted(chat, None).unwrap();
        assert!(core.chat_prefs().is_empty());
    }
}

#[cfg(test)]
mod storage {
    use super::*;

    /// A database that cannot be opened is reported, and nothing is sent as if it were kept
    /// (recheck R02).
    #[test]
    fn nothing_is_sent_without_storage() {
        let file = std::env::temp_dir().join(format!("lichen-not-a-dir-{}", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let core = LichenCore::new(
            "t".into(),
            vec![1; 32],
            vec![150; 32],
            Some(file.display().to_string()),
        )
        .unwrap();
        assert!(!core.storage_ok());
        assert!(matches!(
            core.send_text(vec![9; 32], "hello".into(), 1, 1),
            Err(LichenError::Storage)
        ));
        let _ = std::fs::remove_file(&file);
        let fine = LichenCore::new("t".into(), vec![1; 32], vec![151; 32], None).unwrap();
        assert!(fine.storage_ok());
    }
}

#[cfg(test)]
mod reactions {
    use super::*;

    /// A reaction may arrive before the message it reacts to (§6.2): it shows once that
    /// message is there, and it does not appear as a message itself.
    #[test]
    fn a_reaction_before_its_target() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![140; 32], None).unwrap();
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        let text = Content::Text {
            sent_at: 5,
            text: "hi".into(),
        };
        let target = message_ref(&[7; 32], &text.encode());
        let reaction = received(
            [9; 32],
            Content::Reaction {
                sent_at: 6,
                target,
                emoji: "👍".into(),
            },
        );
        assert!(!i.on_event(&reaction), "not a message");
        assert!(i.extra.iter().any(|e| matches!(e,
            LichenEvent::Reacted { emoji, target: None, .. } if emoji == "👍")));
        assert!(i.on_event(&received([9; 32], text)));
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: vec![9; 32] }, None, 10)
            .unwrap();
        assert_eq!(h.len(), 1);
        assert!(h[0].referable);
        assert_eq!(
            h[0].reactions,
            vec![ReactionCount {
                emoji: "👍".into(),
                count: 1,
                mine: false
            }]
        );
    }

    /// A reference is computed on the bytes as sent (§6.2): a text whose bytes decode lossily
    /// (an invalid UTF-8 byte) still gets the reaction its author's phone aimed at it
    /// (review of 2026-11-15, SR-20).
    #[test]
    fn a_reference_uses_the_bytes_as_sent() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![147; 32], None).unwrap();
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        let mut raw = vec![lc_proto::content::TEXT];
        raw.extend_from_slice(&5u32.to_le_bytes());
        raw.extend_from_slice(b"caf\xe9");
        let content = Content::decode(&raw).unwrap();
        assert_ne!(content.encode(), raw, "decodes lossily");
        assert!(i.on_event(&Event::Received {
            from: [9; 32],
            content,
            plaintext: raw.clone(),
            snr4: 0,
            rssi: 0,
        }));
        let target = message_ref(&[7; 32], &raw);
        assert!(!i.on_event(&received(
            [9; 32],
            Content::Reaction {
                sent_at: 6,
                target,
                emoji: "👍".into(),
            },
        )));
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: vec![9; 32] }, None, 10)
            .unwrap();
        assert_eq!(h[0].reactions.len(), 1, "the reaction found its message");
    }

    /// Edits and deletes (§6.4): only the author's apply, the latest edit shows, a delete
    /// takes the message back, also when it arrives first.
    #[test]
    fn edits_and_deletes_from_the_author_only() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![142; 32], None).unwrap();
        let mut i = core.lock();
        let gid = [4; 16];
        let now = u32::try_from(unix_ms() / 1000).unwrap();
        let text = |t: &str| Content::Text {
            sent_at: now,
            text: t.into(),
        };
        let group = |from: u8, content: Content| {
            group_message(gid, [from; 32], [from + 100; 32], content, false)
        };
        let first = text("at the col at 9");
        let target = message_ref(&[109; 32], &first.encode());
        assert!(i.on_event(&group(9, first)));
        // Another member cannot change it; its author can.
        let edit = |ts: u32, t: &str| Content::Edit {
            sent_at: ts,
            target,
            text: t.into(),
        };
        assert!(!i.on_event(&group(8, edit(now + 5, "forged"))));
        assert!(!i.on_event(&group(9, edit(now + 10, "at the col at 10"))));
        assert!(!i.on_event(&group(9, edit(now + 8, "older"))));
        drop(i);
        let chat = ChatRef::Group { gid: gid.to_vec() };
        let h = core.history(chat.clone(), None, 10).unwrap();
        assert_eq!(
            (h[0].message.clone(), h[0].edited),
            (
                Message::Text {
                    sent_at: now,
                    text: "at the col at 10".into()
                },
                true
            )
        );
        assert!(!h[0].editable, "not ours");
        // Its author takes it back.
        let mut i = core.lock();
        assert!(!i.on_event(&group(
            9,
            Content::Delete {
                sent_at: now + 20,
                target
            }
        )));
        drop(i);
        let h = core.history(chat.clone(), None, 10).unwrap();
        assert_eq!(h[0].message, Message::Deleted { sent_at: now });
        // A delete before its message: the message arrives taken back.
        let second = text("second thoughts");
        let later = message_ref(&[109; 32], &second.encode());
        let mut i = core.lock();
        assert!(!i.on_event(&group(
            9,
            Content::Delete {
                sent_at: now + 30,
                target: later
            }
        )));
        assert!(i.on_event(&group(9, second)));
        drop(i);
        let h = core.history(chat, None, 10).unwrap();
        assert_eq!(h[1].message, Message::Deleted { sent_at: now });
    }

    /// Forwarding (ADR-0027): the current text goes as a new message of ours; a sticker
    /// never goes to a MeshCore chat.
    #[test]
    fn forwarding() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![144; 32], None).unwrap();
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        let gid = [6; 16];
        let now = u32::try_from(unix_ms() / 1000).unwrap();
        let text = Content::Text {
            sent_at: now,
            text: "trail closed".into(),
        };
        let target = message_ref(&[110; 32], &text.encode());
        let group = |content: Content| group_message(gid, [10; 32], [110; 32], content, false);
        assert!(i.on_event(&group(text)));
        assert!(!i.on_event(&group(Content::Edit {
            sent_at: now + 1,
            target,
            text: "trail closed after the bridge".into(),
        })));
        assert!(i.on_event(&group(Content::Sticker {
            sent_at: now,
            set: 1,
            emoji: "🥳".into(),
        })));
        drop(i);
        let h = core
            .history(ChatRef::Group { gid: gid.to_vec() }, None, 10)
            .unwrap();
        let bob = ChatRef::Contact { key: vec![9; 32] };
        let f = core.forward(h[0].id, bob.clone(), now + 5, 0).unwrap();
        assert!(f.outgoing);
        assert_eq!(
            f.message,
            Message::Text {
                sent_at: now + 5,
                text: "trail closed after the bridge".into()
            }
        );
        assert!(core.forward(h[1].id, bob, now + 6, 0).is_ok());
        let plain = ChatRef::Mesh { key: vec![9; 32] };
        assert!(core.forward(h[1].id, plain, now + 7, 0).is_err());
    }

    /// Our own: editable and deletable for 24 hours; deleted here at once.
    #[test]
    fn our_edit_and_delete() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![143; 32], None).unwrap();
        let gid = vec![5u8; 16];
        let now = u32::try_from(unix_ms() / 1000).unwrap();
        // Ours, as the outbox stores them (the radio is not needed to change them).
        let ours = |text: &str, sent_at: u32| {
            let i = core.lock();
            let content = Content::Text {
                sent_at,
                text: text.into(),
            };
            let row = i
                .store
                .as_ref()
                .unwrap()
                .insert(&store::Message {
                    id: 0,
                    chat: Chat::Group([5; 16]),
                    outgoing: true,
                    author: None,
                    msg_ref: Some(message_ref(&[1; 32], &content.encode())),
                    content,
                    at_ms: unix_ms(),
                    status: Status::Flooded,
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                })
                .unwrap();
            i.present(row)
        };
        let m = ours("meet at 9", now);
        assert!(m.editable && m.deletable);
        let e = core.send_edit(m.id, "meet at 10".into(), 0).unwrap();
        assert!(e.edited);
        assert_eq!(
            e.message,
            Message::Text {
                sent_at: now,
                text: "meet at 10".into()
            }
        );
        let d = core.send_delete(m.id, 0).unwrap();
        assert_eq!(d.message, Message::Deleted { sent_at: now });
        assert!(!d.deletable);
        assert!(core.send_edit(m.id, "again".into(), 0).is_err());
        // Too old to change.
        let old = ours("yesterday", now - 90_000);
        assert!(!old.editable && !old.deletable);
        core.delete_for_me(old.id).unwrap();
        let h = core.history(ChatRef::Group { gid }, None, 10).unwrap();
        assert!(h.iter().all(|x| x.id != old.id));
    }

    /// A group message's reference is computed with its author's identity, as its author
    /// computed it (§6.2), not with the node the member is known by: a reaction made on the
    /// author's phone finds it.
    #[test]
    fn group_references_use_the_identity() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![141; 32], None).unwrap();
        let mut i = core.lock();
        let gid = [3; 16];
        let text = Content::Text {
            sent_at: 5,
            text: "hi".into(),
        };
        assert!(i.on_event(&group_message(gid, [9; 32], [7; 32], text.clone(), false)));
        let by_identity = message_ref(&[7; 32], &text.encode());
        let st = i.store.as_ref().unwrap();
        assert!(
            st.message_by_ref(&Chat::Group(gid), &by_identity)
                .unwrap()
                .is_some()
        );
        // The author's own reaction, computed on its phone, attaches to it.
        assert!(!i.on_event(&group_message(
            gid,
            [9; 32],
            [7; 32],
            Content::Reaction {
                sent_at: 6,
                target: by_identity,
                emoji: "👍".into(),
            },
            false
        )));
        drop(i);
        let h = core
            .history(ChatRef::Group { gid: vec![3; 16] }, None, 10)
            .unwrap();
        assert_eq!(h[0].reactions.len(), 1);
    }
}

#[cfg(test)]
mod receiver {
    use super::*;

    /// A copy of a message already stored is dropped, direct or group (§6.1).
    #[test]
    fn resends_are_dropped() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![120; 32], None).unwrap();
        let text = |t: &str| Content::Text {
            sent_at: 5,
            text: t.into(),
        };
        let mut i = core.lock();
        let direct = received([9; 32], text("hi"));
        assert!(i.on_event(&direct));
        assert!(!i.on_event(&direct));
        let group = |from: u8| group_message([3; 16], [from; 32], [from; 32], text("hi"), false);
        assert!(i.on_event(&group(1)));
        assert!(!i.on_event(&group(1)));
        // Another member's identical text, or another second, is another message.
        assert!(i.on_event(&group(2)));
        assert!(i.on_event(&received(
            [9; 32],
            Content::Text {
                sent_at: 6,
                text: "hi".into()
            }
        )));
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: vec![9; 32] }, None, 10)
            .unwrap();
        assert_eq!(h.len(), 2);
    }
}

#[cfg(test)]
mod file_names {
    use super::{fit_file_name, small_file};

    /// A track shaped like a hiking route export, `points` long.
    fn track(points: u32) -> Vec<u8> {
        let mut s = String::from("<?xml version=\"1.0\"?>\n<gpx><trk><trkseg>\n");
        let (mut lat, mut lon, mut ele) = (45_070_000i64, 5_520_000i64, 1200i64);
        for i in 0..i64::from(points) {
            lat += (i * 37) % 23 - 9;
            lon += (i * 53) % 29 - 11;
            ele += (i * 11) % 7 - 3;
            s.push_str(&format!(
                "<trkpt lat=\"{}.{:06}\" lon=\"{}.{:06}\"><ele>{ele}</ele></trkpt>\n",
                lat / 1_000_000,
                lat % 1_000_000,
                lon / 1_000_000,
                lon % 1_000_000
            ));
        }
        s.push_str("</trkseg></trk></gpx>\n");
        s.into_bytes()
    }

    /// Sizes on the air of typical small files (ADR-0023, `docs/measurements.md`):
    /// `cargo test -p lc-ffi --release --lib -- --ignored file_sizes --nocapture`.
    #[test]
    #[ignore]
    fn file_sizes() {
        let prose = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/spec/design.md"
        ))
        .unwrap_or_default();
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        let pdf: Vec<u8> = (0..6_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect();
        for (name, data) in [
            ("track.gpx", track(400)),
            ("note.txt", prose.get(..6_000).unwrap_or(&prose).to_vec()),
            ("scan.pdf", pdf),
        ] {
            let len = data.len();
            match small_file(name.into(), data, 1) {
                Some((_, d)) => println!(
                    "{name}: {len} bytes, {} on the air ({})",
                    d.len(),
                    if d.get(5) == Some(&3) {
                        "compressed"
                    } else {
                        "as is"
                    }
                ),
                None => println!("{name}: {len} bytes, does not fit"),
            }
        }
    }

    #[test]
    fn compressed_files_fit_over_lora() {
        // A 25 KB track goes over LoRa once compressed (ADR-0023).
        let gpx = track(330);
        assert!(gpx.len() > 20_000, "{}", gpx.len());
        let (_, d) = small_file("visorando.gpx".into(), gpx, 1).expect("fits");
        assert_eq!(d.get(5), Some(&3));
        assert!(d.len() < 8_192, "{}", d.len());
        // 9 KB that do not compress: too large. 64 KiB + 1: too large however compressible.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let noise: Vec<u8> = (0..9_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect();
        assert!(small_file("a.jpg".into(), noise, 1).is_none());
        assert!(small_file("zeros.bin".into(), vec![0; 65_537], 1).is_none());
        assert!(small_file("zeros.bin".into(), vec![0; 65_536], 1).is_some());
        assert!(small_file("empty.txt".into(), Vec::new(), 1).is_none());
    }

    #[test]
    fn long_names_keep_their_extension() {
        assert_eq!(fit_file_name("topo.gpx".into(), 32), "topo.gpx");
        let cut = fit_file_name("Compte rendu de la réunion du conseil.pdf".into(), 32);
        assert!(cut.len() <= 32 && cut.ends_with("….pdf"), "{cut}");
        assert_eq!(fit_file_name("../../etc/passwd".into(), 64), "etcpasswd");
        // Never a path component of its own, nor a hidden file (recheck R01).
        for hostile in [".", "..", "...", " .. ", "/..", "..\\", "\u{0}..", "\n", ""] {
            assert_eq!(fit_file_name(hostile.into(), 64), "", "{hostile:?}");
        }
        assert_eq!(fit_file_name(".bashrc".into(), 64), "bashrc");
        assert_eq!(fit_file_name("a\u{7}b:c.txt".into(), 64), "abc.txt");
        assert_eq!(fit_file_name("..pdf".into(), 64), "pdf");
        let no_ext = fit_file_name("x".repeat(80), 64);
        assert_eq!(no_ext.len(), 64);
        let accents = fit_file_name(format!("{}.txt", "é".repeat(40)), 32);
        assert!(accents.len() <= 32 && accents.ends_with(".txt"));
    }
}

/// Review of 2026-11-15, SR-23: a contact card proves who someone is only when it is scanned
/// in person. A pasted one adds an unverified contact, and never replaces an identity already
/// known: that one waits for the user.
#[cfg(test)]
mod cards {
    use super::*;

    fn core() -> std::sync::Arc<LichenCore> {
        LichenCore::new("t".into(), vec![1; 32], vec![101; 32], None).unwrap()
    }

    fn card(identity: u8, node: u8) -> String {
        lc_core::identity::Card {
            identity: [identity; 32],
            node: [node; 32],
            name: "Camille".into(),
        }
        .encode()
    }

    fn state(c: &LichenCore, node: u8) -> (Option<Vec<u8>>, bool) {
        let k = c
            .contacts()
            .into_iter()
            .find(|k| k.key == vec![node; 32])
            .unwrap();
        (k.identity, k.verified)
    }

    #[test]
    fn a_pasted_card_does_not_verify() {
        let c = core();
        c.add_contact_card(card(7, 70), false).unwrap();
        assert_eq!(state(&c, 70), (Some(vec![7; 32]), false));
        // Scanning the same card in person then verifies it.
        c.add_contact_card(card(7, 70), true).unwrap();
        assert_eq!(state(&c, 70), (Some(vec![7; 32]), true));
    }

    #[test]
    fn a_pasted_card_does_not_replace_an_identity() {
        let c = core();
        c.add_contact_card(card(7, 70), true).unwrap();
        c.add_contact_card(card(8, 70), false).unwrap();
        assert_eq!(state(&c, 70), (Some(vec![7; 32]), true));
        assert_eq!(c.pending_identity(vec![70; 32]).unwrap(), Some(vec![8; 32]));
        // Scanned in person, the new card is the contact, verified.
        c.add_contact_card(card(8, 70), true).unwrap();
        assert_eq!(state(&c, 70), (Some(vec![8; 32]), true));
        assert_eq!(c.pending_identity(vec![70; 32]).unwrap(), None);
    }
}

/// Review of 2026-11-15, SR-15: a modified client can date an edit within 24 hours of its
/// message whenever it likes. The receiver also counts from when the message arrived.
#[cfg(test)]
mod edit_window {
    use super::*;

    #[test]
    fn an_edit_long_after_arrival_is_not_applied() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![143; 32], None).unwrap();
        let mut i = core.lock();
        let bob = [9u8; 32];
        let now = u32::try_from(unix_ms() / 1000).unwrap();
        let first = Content::Text {
            sent_at: now,
            text: "see you at 9".into(),
        };
        // It arrived two days ago (here), though its author dates the edit within the day.
        let target = message_ref(&bob, &first.encode());
        i.record(store::Message {
            id: 0,
            chat: Chat::Contact(bob),
            outgoing: false,
            author: None,
            content: first,
            at_ms: unix_ms() - 2 * 86_400_000,
            status: Status::Received,
            fragments: 0,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: Some(target),
        });
        i.on_amendment(
            Chat::Contact(bob),
            target,
            &bob,
            Some("see you never"),
            now + 60,
        );
        drop(i);
        let h = core
            .history(ChatRef::Contact { key: bob.to_vec() }, None, 10)
            .unwrap();
        assert!(!h[0].edited);
    }
}

/// Review of 2026-11-15, SR-30: a verified contact's node renames itself; the app keeps the
/// name it had when the user verified it.
#[cfg(test)]
mod names {
    use super::*;

    #[test]
    fn a_verified_contact_keeps_its_name() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![144; 32], None).unwrap();
        let card = lc_core::identity::Card {
            identity: [7; 32],
            node: [70; 32],
            name: "Camille".into(),
        }
        .encode();
        core.add_contact_card(card, true).unwrap();
        // Her node now advertises another name.
        core.lock().core.add_contact([70; 32], "Julien", None);
        let c = core
            .contacts()
            .into_iter()
            .find(|c| c.key == vec![70; 32])
            .unwrap();
        assert_eq!((c.name.as_str(), c.verified), ("Camille", true));
        assert_eq!(c.node_name, "Julien");
    }

    /// The rename notices of `chat`: the name shown before, the new name.
    fn renames(core: &LichenCore, chat: ChatRef) -> Vec<(String, String)> {
        core.history(chat, None, 50)
            .unwrap()
            .into_iter()
            .filter_map(|m| match m.message {
                Message::Notice {
                    kind: 12,
                    actor_name,
                    text,
                    ..
                } => Some((actor_name, text)),
                _ => None,
            })
            .collect()
    }

    /// The rest of SR-30: the name shown stays, but the conversation says that the radio now
    /// goes by another one; only conversations we have get the line, once per change.
    #[test]
    fn a_rename_is_noted_in_the_conversation() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![146; 32], None).unwrap();
        let card = lc_core::identity::Card {
            identity: [7; 32],
            node: [70; 32],
            name: "Camille".into(),
        }
        .encode();
        core.add_contact_card(card, true).unwrap();
        {
            let mut i = core.lock();
            // A stranger's radio, no conversation with it.
            i.core.add_contact([71; 32], "Lucie", None);
            i.save_contacts();
            i.record(store::Message {
                id: 0,
                chat: Chat::Contact([70; 32]),
                outgoing: false,
                author: None,
                content: Content::Text {
                    sent_at: 1,
                    text: "hello".into(),
                },
                at_ms: unix_ms(),
                status: Status::Received,
                fragments: 0,
                airtime_ms: 0,
                snr4: None,
                read: true,
                msg_ref: None,
            })
            .unwrap();
            i.core.add_contact([70; 32], "Julien", None);
            i.core.add_contact([71; 32], "Lucie 2", None);
            i.save_contacts();
            // Saved: no second line for the same change.
            i.save_contacts();
        }
        let camille = ChatRef::Contact { key: vec![70; 32] };
        assert_eq!(
            renames(&core, camille),
            vec![("Camille".to_owned(), "Julien".to_owned())]
        );
        assert!(renames(&core, ChatRef::Contact { key: vec![71; 32] }).is_empty());
        assert!(renames(&core, ChatRef::Mesh { key: vec![70; 32] }).is_empty());
    }
}

/// Review of 2026-11-15, SR-28: a deleted announcement's offer and download go too. The app
/// is told which content to drop.
#[cfg(test)]
mod media_gone {
    use super::*;
    use lc_core::content::Announce;

    #[test]
    fn deleting_an_announcement_drops_its_content() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![145; 32], None).unwrap();
        let bob = [9u8; 32];
        let row = {
            let mut i = core.lock();
            i.record(store::Message {
                id: 0,
                chat: Chat::Contact(bob),
                outgoing: false,
                author: None,
                content: Content::Announce(Announce {
                    sent_at: 1,
                    media_id: [0; 8],
                    kind: 3,
                    hash: [0x77; 32],
                    size: 1_000,
                    providers: vec![(1, [5; 32])],
                    name: "track.gpx".into(),
                }),
                at_ms: unix_ms(),
                status: Status::Received,
                fragments: 0,
                airtime_ms: 0,
                snr4: None,
                read: true,
                msg_ref: None,
            })
            .unwrap()
        };
        while core.poll_event().is_some() {}
        core.delete_for_me(row.id).unwrap();
        let events: Vec<LichenEvent> = std::iter::from_fn(|| core.poll_event()).collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, LichenEvent::MediaGone { hash } if hash == &vec![0x77; 32])),
            "{events:?}"
        );
    }
    fn announce(core: &LichenCore, chat: Chat, outgoing: bool) -> i64 {
        let mut i = core.lock();
        i.record(store::Message {
            id: 0,
            chat,
            outgoing,
            author: None,
            content: Content::Announce(Announce {
                sent_at: 1,
                media_id: [0; 8],
                kind: 1,
                hash: [0x66; 32],
                size: 2_000,
                providers: vec![(1, [5; 32])],
                name: "photo.jpg".into(),
            }),
            at_ms: unix_ms(),
            status: if outgoing {
                Status::Delivered
            } else {
                Status::Received
            },
            fragments: 0,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: None,
        })
        .unwrap()
        .id
    }

    /// Second read of 2026-11-17, N7: a contact announces again content we sent (the same
    /// hash), then deletes their message. Our offer and our copy stay while our own message
    /// announces it; they go with the last message that does.
    #[test]
    fn someone_elses_delete_leaves_our_media() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![146; 32], None).unwrap();
        let (camille, bob) = ([8u8; 32], [9u8; 32]);
        let ours = announce(&core, Chat::Contact(camille), true);
        let theirs = announce(&core, Chat::Contact(bob), false);
        while core.poll_event().is_some() {}
        let gone = |core: &LichenCore| {
            std::iter::from_fn(|| core.poll_event())
                .any(|e| matches!(e, LichenEvent::MediaGone { hash } if hash == vec![0x66; 32]))
        };
        core.delete_for_me(theirs).unwrap();
        assert!(!gone(&core), "their delete took our media");
        core.delete_for_me(ours).unwrap();
        assert!(gone(&core), "the last announcement went, its media stayed");
    }
}

/// Review of 2026-11-15, SR-35: a file name cannot reorder itself on screen.
#[cfg(test)]
mod bidi_names {
    use super::*;

    #[test]
    fn bidirectional_controls_are_dropped() {
        let spoofed = "photo\u{202E}gpj.exe";
        assert_eq!(super::super::message::shown_name(spoofed), "photogpj.exe");
        assert_eq!(fit_file_name(spoofed.into(), 64), "photogpj.exe");
    }
}

mod timers {
    use super::*;

    /// The timer notices of `chat`: the seconds, and whether Bob set them.
    fn notices(core: &LichenCore, chat: &ChatRef) -> Vec<(String, bool)> {
        core.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .filter_map(|m| match m.message {
                Message::Notice {
                    kind: 11,
                    text,
                    actor,
                    ..
                } => Some((text, actor == [9; 32])),
                _ => None,
            })
            .collect()
    }

    /// Disappearing messages (§6.5): the newest setting holds, each change is noted, and
    /// messages stored under a timer go when it runs out, with an event for the app.
    #[test]
    fn a_timer_from_the_contact_and_the_sweep() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![145; 32], None).unwrap();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        let now = u32::try_from(unix_ms() / 1000).unwrap();
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        let timer =
            |seconds: u32, sent_at: u32| received([9; 32], Content::Timer { sent_at, seconds });
        assert!(!i.on_event(&timer(300, now)), "not a message");
        assert!(!i.on_event(&timer(60, now - 10)), "older: ignored");
        assert!(!i.on_event(&timer(300, now)), "a resend: no second notice");
        drop(i);
        assert_eq!(core.disappearing(chat.clone()).unwrap(), 300);
        assert_eq!(notices(&core, &chat), vec![("300".into(), true)]);
        // A message stored under the timer, long ago: the next sweep takes it.
        let mut i = core.lock();
        assert!(i.on_event(&received(
            [9; 32],
            Content::Text {
                sent_at: now,
                text: "burn after reading".into(),
            }
        )));
        assert!(i.next_vanish().is_some());
        // One stored under the same timer six minutes ago: its time is up.
        let old = i
            .store
            .as_ref()
            .unwrap()
            .insert(&store::Message {
                id: 0,
                chat: Chat::Contact([9; 32]),
                outgoing: false,
                author: None,
                content: Content::Text {
                    sent_at: now - 360,
                    text: "earlier".into(),
                },
                at_ms: unix_ms() - 360_000,
                status: Status::Received,
                fragments: 1,
                airtime_ms: 0,
                snr4: None,
                read: true,
                msg_ref: None,
            })
            .unwrap();
        let id = old.id;
        i.extra.clear();
        i.vanish();
        assert!(i.extra.iter().any(
            |e| matches!(e, LichenEvent::MessageRemoved { id: x, chat: c } if *x == id && *c == chat)
        ));
        drop(i);
        let h = core.history(chat.clone(), None, 10).unwrap();
        assert!(h.iter().all(|m| m.id != id), "gone");
        assert!(
            h.iter().any(
                |m| matches!(&m.message, Message::Text { text, .. } if text == "burn after reading")
            ),
            "its five minutes are not over"
        );
        assert_eq!(notices(&core, &chat).len(), 1, "the notice stays");
    }

    /// Ours: a lichen conversation is told and our setting is kept until it goes; a MeshCore
    /// chat keeps it here; more than four weeks is refused.
    #[test]
    fn our_timer() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![146; 32], None).unwrap();
        let bob = ChatRef::Contact { key: vec![9; 32] };
        let plain = ChatRef::Mesh { key: vec![9; 32] };
        core.set_disappearing(bob.clone(), 3_600).unwrap();
        core.set_disappearing(bob.clone(), 3_600).unwrap();
        assert_eq!(core.disappearing(bob.clone()).unwrap(), 3_600);
        assert_eq!(notices(&core, &bob), vec![("3600".into(), false)]);
        {
            let i = core.lock();
            let pending = i.store.as_ref().unwrap().pending_timers().unwrap();
            assert_eq!(pending.len(), 1, "kept until it reaches Bob");
        }
        core.set_disappearing(plain.clone(), 300).unwrap();
        {
            let i = core.lock();
            let pending = i.store.as_ref().unwrap().pending_timers().unwrap();
            assert_eq!(pending.len(), 1, "nothing to tell a MeshCore chat");
        }
        assert_eq!(core.disappearing(plain).unwrap(), 300);
        assert!(core.set_disappearing(bob, 29 * 86_400).is_err());
    }
}

/// The radio's settings as the apps read them (protocol §1.1): telemetry modes from their
/// packed byte, the GPS from the board's variables.
#[cfg(test)]
mod radio_settings {
    use crate::api_radio::RadioSettings;
    use lc_meshcore::OtherParams;

    #[test]
    fn telemetry_and_gps_for_the_apps() {
        let core = lc_core::radio::RadioSettings {
            other: OtherParams {
                manual_add: true,
                telemetry: 2 << 4 | 1 << 2,
                advert_loc_policy: 1,
                multi_acks: 0,
            },
            vars: vec![
                ("gps".into(), "1".into()),
                ("gps_interval".into(), "300".into()),
            ],
            autoadd: Some((0x03, 2)),
            ..Default::default()
        };
        let r = RadioSettings::from(&core);
        assert_eq!(
            (
                r.telemetry.base,
                r.telemetry.location,
                r.telemetry.environment
            ),
            (0, 1, 2)
        );
        assert!(r.position_in_adverts && r.manual_add);
        assert_eq!((r.gps, r.gps_interval_s), (Some(true), Some(300)));
        assert_eq!((r.autoadd_mask, r.autoadd_max_hops), (Some(3), Some(2)));
        let none = RadioSettings::from(&lc_core::radio::RadioSettings::default());
        assert_eq!(
            (none.gps, none.battery_mv, none.autoadd_mask),
            (None, None, None)
        );
    }
}

/// History from the MeshCore app's export (ADR-0033): into the MeshCore chats, with the
/// people's names, and nothing twice.
#[cfg(test)]
mod import_meshcore {
    use super::*;

    const ME: [u8; 32] = [0xAA; 32];
    const MARC: [u8; 32] = [0x4D; 32];

    fn hex(k: &[u8]) -> String {
        k.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn export() -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("lichen-ffi-import-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            r#"CREATE TABLE "contact_messages" ("id" INTEGER PRIMARY KEY, "status" TEXT NOT NULL, "to" TEXT NOT NULL, "from" TEXT NOT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL);
               CREATE TABLE "channel_messages" ("id" INTEGER PRIMARY KEY, "channel_secret" BLOB NULL, "from" TEXT NULL, "txt_type" INTEGER NOT NULL, "sender_timestamp" INTEGER NOT NULL, "text" TEXT NOT NULL, "timestamp" INTEGER NOT NULL, "snr" REAL NULL);
               CREATE TABLE "contacts" ("id" INTEGER PRIMARY KEY, "public_key" BLOB NOT NULL, "adv_name" TEXT NOT NULL, "custom_name" TEXT NULL);"#,
        )
        .unwrap();
        db.execute(
            r#"INSERT INTO contact_messages (status, "to", "from", txt_type, sender_timestamp, text, timestamp) VALUES ('received', ?1, ?2, 0, 1793000000, 'At the refuge?', 1793000000250)"#,
            rusqlite::params![hex(&ME), hex(&MARC)],
        )
        .unwrap();
        db.execute(
            r#"INSERT INTO channel_messages (channel_secret, "from", txt_type, sender_timestamp, text, timestamp) VALUES (?1, NULL, 0, 1793000010, 'Lucie: Fog on the ridge', 1793000010000)"#,
            rusqlite::params![[0x8Bu8; 16].as_slice()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO contacts (public_key, adv_name, custom_name) VALUES (?1, 'MARC-R1', 'Marc')",
            rusqlite::params![MARC.as_slice()],
        )
        .unwrap();
        path
    }

    #[test]
    fn imported_once_with_names() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![147; 32], None).unwrap();
        let path = export();
        let first = core
            .import_meshcore_app(path.display().to_string())
            .unwrap();
        assert_eq!(
            (
                first.direct,
                first.channel,
                first.already_here,
                first.skipped
            ),
            (1, 1, 0, 0)
        );
        let again = core
            .import_meshcore_app(path.display().to_string())
            .unwrap();
        assert_eq!((again.direct, again.channel, again.already_here), (0, 0, 2));
        let _ = std::fs::remove_file(&path);
        let marc = core
            .contacts()
            .into_iter()
            .find(|c| c.key == MARC.to_vec())
            .unwrap();
        assert_eq!(marc.name, "Marc");
        let history = core
            .history(ChatRef::Mesh { key: MARC.to_vec() }, None, 10)
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(
            matches!(&history[0].message, Message::Text { text, .. } if text == "At the refuge?")
        );
        // Not a MeshCore export.
        let junk = std::env::temp_dir().join(format!("lichen-ffi-junk-{}.db", std::process::id()));
        std::fs::write(&junk, b"nothing").unwrap();
        assert!(matches!(
            core.import_meshcore_app(junk.display().to_string()),
            Err(LichenError::NotAnExport)
        ));
        let _ = std::fs::remove_file(&junk);
    }
}

/// The demo's radio reports its settings to the app (ADR-0032): what SELF_INFO carries too.
#[cfg(test)]
mod demo_radio_settings {
    use super::*;

    #[test]
    fn the_demo_radio_reports_all_its_settings() {
        let demo = crate::DemoNetwork::new();
        let core = LichenCore::new("t".into(), vec![9; 32], vec![148; 32], None).unwrap();
        // The settings screen asks for the battery as it appears, maybe before the connection.
        core.refresh_radio();
        core.connect(0);
        let mut now = 0;
        while now < 20_000 {
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
        let r = core.radio_settings();
        assert_eq!((r.freq_khz, r.bw_hz, r.sf), (869_618, 62_500, 8), "{r:?}");
        assert_eq!(r.max_tx_dbm, 22);
        assert_eq!(r.gps, Some(false));
        assert_eq!(r.battery_mv, Some(4_012));
    }
}

#[cfg(test)]
mod backup {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("lichen-ffi-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn text(t: &str) -> store::Message {
        store::Message {
            id: 0,
            chat: Chat::Contact([0x4D; 32]),
            outgoing: false,
            author: None,
            content: Content::Text {
                sent_at: 1_793_000_000,
                text: t.into(),
            },
            at_ms: 1_793_000_000_000,
            status: Status::Received,
            fragments: 1,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: None,
        }
    }

    /// ADR-0037: exported on one phone, restored on another with the words; the app then
    /// starts with that identity and finds its history and media.
    #[test]
    fn exported_then_restored_elsewhere() {
        let (a_dir, b_dir) = (dir("a"), dir("b"));
        let a = LichenCore::new(
            "t".into(),
            vec![1; 32],
            vec![150; 32],
            Some(a_dir.display().to_string()),
        )
        .unwrap();
        a.lock()
            .store
            .as_ref()
            .unwrap()
            .insert(&text("At the refuge?"))
            .unwrap();
        std::fs::create_dir_all(a_dir.join("downloads")).unwrap();
        std::fs::write(a_dir.join("downloads").join("ab12.jpg"), b"photo").unwrap();
        let file = a_dir.join("lichen.backup").display().to_string();
        let made = a
            .export_backup(file.clone(), br#"{"lichen.tor":true}"#.to_vec())
            .unwrap();
        assert_eq!((made.messages, made.files), (1, 1));
        assert!(!a_dir.join("backup-work").join("backup.sqlite").exists());

        let b = LichenCore::new(
            "t".into(),
            vec![2; 32],
            vec![151; 32],
            Some(b_dir.display().to_string()),
        )
        .unwrap();
        let other = lc_core::identity::recovery_phrase(&[7; 32]);
        let into = b_dir.display().to_string();
        assert_eq!(
            b.restore_backup(file.clone(), other, into.clone()),
            Err(LichenError::WrongBackupWords)
        );
        assert_eq!(
            b.restore_backup(file.clone(), vec!["lichen".into(); 24], into.clone()),
            Err(LichenError::BadCode)
        );
        assert_eq!(
            b.restore_backup(
                b_dir.join("none").display().to_string(),
                a.recovery_phrase(),
                into.clone()
            ),
            Err(LichenError::NotABackup)
        );
        let r = b
            .restore_backup(file.clone(), a.recovery_phrase(), into.clone())
            .unwrap();
        assert_eq!(r.identity_seed, vec![150; 32]);
        assert_eq!(r.settings, br#"{"lichen.tor":true}"#);
        assert_eq!((r.messages, r.files), (1, 1));
        assert!(b.storage_ok(), "another identity's store stays open");
        assert_eq!(
            std::fs::read(b_dir.join("downloads").join("ab12.jpg")).unwrap(),
            b"photo"
        );

        let c = LichenCore::new(
            "t".into(),
            vec![3; 32],
            r.identity_seed,
            Some(b_dir.display().to_string()),
        )
        .unwrap();
        let chat = ChatRef::Contact {
            key: vec![0x4D; 32],
        };
        let h = c.history(chat, None, 10).unwrap();
        assert!(
            matches!(&h[..], [m] if matches!(&m.message, Message::Text { text, .. } if text == "At the refuge?"))
        );
        // Restored again over its own open store: that store is closed first.
        c.restore_backup(file.clone(), a.recovery_phrase(), into.clone())
            .unwrap();
        assert!(!c.storage_ok());
        // A demo core exports nothing, but can restore into the app's folder.
        let demo = LichenCore::new("t".into(), vec![4; 32], vec![152; 32], None).unwrap();
        assert_eq!(
            demo.export_backup("x".into(), Vec::new()),
            Err(LichenError::NotAllowed)
        );
        assert!(demo.restore_backup(file, a.recovery_phrase(), into).is_ok());
        for d in [a_dir, b_dir] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// ADR-0036: a database key is used only with SQLCipher built in; otherwise nothing is
    /// stored, rather than stored in clear.
    #[test]
    fn a_database_key_needs_sqlcipher() {
        let d = dir("key");
        let at = Some(d.display().to_string());
        let c = LichenCore::with_database_key(
            "t".into(),
            vec![1; 32],
            vec![153; 32],
            at.clone(),
            Some(vec![9; 32]),
        )
        .unwrap();
        assert_eq!(c.storage_ok(), database_encryption());
        assert!(
            LichenCore::with_database_key(
                "t".into(),
                vec![1; 32],
                vec![153; 32],
                at.clone(),
                Some(vec![9; 31])
            )
            .is_err()
        );
        // The app could not read its key: no database, rather than a clear one.
        let locked = LichenCore::with_database_key(
            "t".into(),
            vec![1; 32],
            vec![154; 32],
            at,
            Some(Vec::new()),
        )
        .unwrap();
        assert!(!locked.storage_ok());
        let _ = std::fs::remove_dir_all(d);
    }

    /// A plain database waits for no key; discarding a database removes it and its journals.
    #[test]
    fn a_plain_database_waits_for_no_key() {
        let d = dir("plain-db");
        let at = d.display().to_string();
        drop(LichenCore::new("t".into(), vec![1; 32], vec![156; 32], Some(at.clone())).unwrap());
        let file = database_file(
            &d,
            &lc_meshcore::ecdh::public_key(&lc_meshcore::ecdh::expand_seed(&[156; 32])),
        );
        assert!(file.exists());
        assert_eq!(encrypted_database(at.clone(), vec![156; 32]), Ok(false));
        assert_eq!(
            encrypted_database(at.clone(), vec![157; 32]),
            Ok(false),
            "none yet"
        );
        assert_eq!(
            encrypted_database(at.clone(), vec![1; 31]),
            Err(LichenError::InvalidKey)
        );
        discard_database(at.clone(), vec![156; 32]).unwrap();
        assert!(!file.exists());
        discard_database(at, vec![156; 32]).unwrap();
        let _ = std::fs::remove_dir_all(d);
    }

    #[cfg(feature = "sqlcipher")]
    fn keyed(seed: u8, d: &std::path::Path, key: Option<Vec<u8>>) -> Arc<LichenCore> {
        LichenCore::with_database_key(
            "t".into(),
            vec![1; 32],
            vec![seed; 32],
            Some(d.display().to_string()),
            key,
        )
        .unwrap()
    }

    #[cfg(feature = "sqlcipher")]
    fn texts(c: &LichenCore) -> Vec<String> {
        c.history(
            ChatRef::Contact {
                key: vec![0x4D; 32],
            },
            None,
            10,
        )
        .unwrap()
        .into_iter()
        .filter_map(|m| match m.message {
            Message::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
    }

    /// ADR-0036 in the binding layer: a phone's plain database is encrypted at the first start
    /// with a key, then opens only with that key; the app learns that an encrypted database
    /// waits for its key, and can discard it when the key is lost; a backup restored over it
    /// is encrypted again at the next start.
    #[cfg(feature = "sqlcipher")]
    #[test]
    fn the_database_under_its_key() {
        let d = dir("encrypted");
        let (dir_s, seed) = (d.display().to_string(), vec![155; 32]);
        let key = Some(vec![0x6B; 32]);
        // An existing phone: its plain history.
        let plain = keyed(155, &d, None);
        plain
            .lock()
            .store
            .as_ref()
            .unwrap()
            .insert(&text("At the refuge?"))
            .unwrap();
        drop(plain);
        assert_eq!(encrypted_database(dir_s.clone(), seed.clone()), Ok(false));

        let c = keyed(155, &d, key.clone());
        assert!(c.storage_ok());
        assert_eq!(texts(&c), ["At the refuge?"]);
        drop(c);
        assert_eq!(encrypted_database(dir_s.clone(), seed.clone()), Ok(true));
        assert!(
            !keyed(155, &d, Some(vec![0x6C; 32])).storage_ok(),
            "another key"
        );
        assert!(!keyed(155, &d, None).storage_ok(), "no key");
        let c = keyed(155, &d, key.clone());
        assert_eq!(texts(&c), ["At the refuge?"]);

        // A backup of the encrypted store, restored over it: plain until the next start.
        let again = d.join("again.backup").display().to_string();
        c.export_backup(again.clone(), Vec::new()).unwrap();
        c.restore_backup(again, c.recovery_phrase(), dir_s.clone())
            .unwrap();
        drop(c);
        assert_eq!(encrypted_database(dir_s.clone(), seed.clone()), Ok(false));
        assert_eq!(texts(&keyed(155, &d, key.clone())), ["At the refuge?"]);
        assert_eq!(encrypted_database(dir_s.clone(), seed.clone()), Ok(true));

        // The key is lost: the user starts again with an empty history.
        discard_database(dir_s.clone(), seed.clone()).unwrap();
        assert_eq!(encrypted_database(dir_s.clone(), seed), Ok(false));
        let fresh = keyed(155, &d, Some(vec![0x7A; 32]));
        assert!(fresh.storage_ok() && texts(&fresh).is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}

/// Links and places come with UTF-16 offsets, what the apps' strings count (ADR-0034).
#[cfg(test)]
mod text_links {
    use super::*;

    #[test]
    fn offsets_count_utf16_units() {
        // "é" is one unit, "🏔" two.
        let text = "é🏔 https://a.fr/x, 45.1234, 5.6789";
        let links = crate::text_links(text.into());
        assert_eq!(links.len(), 2);
        let units: Vec<u16> = text.encode_utf16().collect();
        let span =
            |l: &TextLink| String::from_utf16(&units[l.start as usize..l.end as usize]).unwrap();
        assert_eq!(span(&links[0]), "https://a.fr/x");
        assert_eq!(
            links[0].target,
            TextTarget::Web {
                url: "https://a.fr/x".into()
            }
        );
        assert_eq!(span(&links[1]), "45.1234, 5.6789");
        assert!(matches!(links[1].target, TextTarget::Place { lat, lon }
            if (lat - 45.1234).abs() < 1e-9 && (lon - 5.6789).abs() < 1e-9));
        assert!(crate::text_links("nothing here".into()).is_empty());
    }
}

/// Blocking and archiving (ADR-0034).
#[cfg(test)]
mod block_archive {
    use super::*;

    fn core_with_bob() -> std::sync::Arc<LichenCore> {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![149; 32], None).unwrap();
        core.lock().core.restore_contact(lc_core::Contact {
            key: [9; 32],
            name: "Bob".into(),
            out_path: None,
            identity: Some([7; 32]),
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        core
    }

    #[test]
    fn a_block_is_listed_and_lifted() {
        let core = core_with_bob();
        core.rename_contact(vec![9; 32], Some("Bobby".into()))
            .unwrap();
        assert!(!core.is_blocked(vec![9; 32]));
        core.block(vec![9; 32], 0).unwrap();
        assert!(core.is_blocked(vec![9; 32]));
        let list = core.blocked();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].name.as_str(), list[0].identity.as_deref()),
            ("Bobby", Some(&[7u8; 32][..]))
        );
        assert_eq!(
            core.send_text(vec![9; 32], "hi".into(), 1, 0).err(),
            Some(LichenError::Blocked)
        );
        assert_eq!(
            core.block(vec![8; 32], 0).err(),
            Some(LichenError::UnknownContact)
        );
        core.unblock(vec![9; 32]).unwrap();
        assert!(!core.is_blocked(vec![9; 32]));
        assert!(core.blocked().is_empty());
    }

    #[test]
    fn a_message_brings_an_archived_chat_back() {
        let core = core_with_bob();
        let chat = ChatRef::Contact { key: vec![9; 32] };
        core.set_archived(chat.clone(), true).unwrap();
        assert!(
            core.chat_prefs()
                .iter()
                .any(|p| p.chat == chat && p.archived)
        );
        let mut i = core.lock();
        assert!(i.on_event(&received(
            [9; 32],
            Content::Text {
                sent_at: 5,
                text: "back".into()
            }
        )));
        assert!(
            i.extra
                .iter()
                .any(|e| matches!(e, LichenEvent::ChatPrefsChanged))
        );
        drop(i);
        assert!(!core.chat_prefs().iter().any(|p| p.archived));
    }
}

/// Media and storage (ADR-0034): old media go as a delete for me does, announcements with
/// their downloads (`MediaGone`), texts stay.
#[cfg(test)]
mod media_storage {
    use super::*;

    #[test]
    fn old_media_are_cleared() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![150; 32], None).unwrap();
        let chat = Chat::Contact([9; 32]);
        let announce = Content::Announce(lc_core::Announce {
            sent_at: 1,
            media_id: [3; 8],
            kind: 3,
            hash: [6; 32],
            size: 10_000,
            providers: vec![(1, [8; 32])],
            name: "topo.pdf".into(),
        });
        {
            let i = core.lock();
            let st = i.store.as_ref().unwrap();
            for (content, at_ms) in [
                (
                    Content::Text {
                        sent_at: 1,
                        text: "old text".into(),
                    },
                    1_000,
                ),
                (announce, 1_000),
                (
                    Content::Voice {
                        sent_at: 1,
                        mode: 2,
                        frames: 2,
                        data: vec![7; 16],
                    },
                    9_000,
                ),
            ] {
                st.insert(&store::Message {
                    id: 0,
                    chat,
                    outgoing: false,
                    author: None,
                    content,
                    at_ms,
                    status: Status::Received,
                    fragments: 1,
                    airtime_ms: 0,
                    snr4: None,
                    read: true,
                    msg_ref: None,
                })
                .unwrap();
            }
        }
        let r = ChatRef::Contact { key: vec![9; 32] };
        assert_eq!(core.chat_media(r.clone(), None, 10).unwrap().len(), 2);
        let usage = core.storage_usage();
        assert_eq!((usage[0].messages, usage[0].media), (3, 2));
        assert_eq!(usage[0].announced, vec![vec![6u8; 32]]);
        assert_eq!(core.clear_media_older_than(5_000).unwrap(), 1);
        assert!(
            core.lock()
                .extra
                .iter()
                .any(|e| matches!(e, LichenEvent::MediaGone { hash } if hash == &[6u8; 32]))
        );
        let left = core.history(r.clone(), None, 10).unwrap();
        assert_eq!(left.len(), 2, "the text and the newer voice message stay");
        assert_eq!(core.chat_media(r, None, 10).unwrap().len(), 1);
    }
}

/// ADR-0035: what a group takes of a voice note, per mode: 5 fragments.
#[cfg(test)]
mod group_voice {
    #[test]
    fn the_longest_note_a_group_takes() {
        assert_eq!(crate::group_voice_max_ms(0), 6_480);
        assert_eq!(crate::group_voice_max_ms(1), 3_760);
        assert_eq!(crate::group_voice_max_ms(2), 2_840);
        assert_eq!(crate::group_voice_max_ms(3), 1_880);
        // 6 s of 700C is what the apps record for a group.
        let v = crate::prepare_voice(vec![0; 48_000], 0);
        assert_eq!(v.frames, 150);
        assert!(crate::voice_datagram_len(&v) as usize + 64 <= 5 * 128);
    }
}

/// ADR-0046 (second read of 2026-11-17, N3): a request is kept apart from the conversation of
/// the contact whose radio it came through, survives a restart, and moves into that
/// conversation only when the user accepts it; blocking it blocks the identity, not the radio.
#[cfg(test)]
mod requests {
    use super::*;

    const BOB: [u8; 32] = [9; 32];
    const EVE: [u8; 32] = [66; 32];

    fn contact(core: &LichenCore) {
        let mut i = core.lock();
        i.core.restore_contact(lc_core::Contact {
            key: BOB,
            name: "Bob".into(),
            out_path: None,
            identity: None,
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
        i.save_contacts();
    }

    fn text(t: &str, sent_at: u32) -> Content {
        Content::Text {
            sent_at,
            text: t.into(),
        }
    }

    fn message(i: &mut Inner, who: [u8; 32], c: Content) {
        i.on_event(&Event::RequestMessage {
            node: BOB,
            identity: who,
            plaintext: c.encode(),
            content: c,
            snr4: 8,
            rssi: -90,
        });
    }

    #[test]
    fn a_request_waits_apart_then_moves_in() {
        let dir = std::env::temp_dir().join(format!("lichen-requests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = Some(dir.to_string_lossy().into_owned());
        let core = LichenCore::new("t".into(), vec![1; 32], vec![161; 32], data.clone()).unwrap();
        contact(&core);
        let bob_chat = ChatRef::Contact { key: BOB.to_vec() };
        {
            let mut i = core.lock();
            for who in [EVE, [70; 32]] {
                // As the core reports it: it holds the request.
                i.core.restore_request(BOB, who);
                i.on_event(&Event::Requested {
                    node: BOB,
                    identity: who,
                });
            }
            message(&mut i, EVE, text("hi, it's Bob", 10));
            message(&mut i, [70; 32], text("it's really me", 11));
            message(
                &mut i,
                [70; 32],
                Content::Reaction {
                    sent_at: 12,
                    target: [1; 8],
                    emoji: "👍".into(),
                },
            );
        }
        let names = |core: &LichenCore| {
            core.requests()
                .into_iter()
                .map(|q| {
                    let lines: Vec<String> = q
                        .messages
                        .iter()
                        .map(|l| match &l.message {
                            Message::Text { text, .. } => text.clone(),
                            m => format!("{m:?}"),
                        })
                        .collect();
                    (q.identity, q.name, lines)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&core),
            vec![
                (EVE.to_vec(), "Bob".into(), vec!["hi, it's Bob".to_owned()]),
                (
                    vec![70; 32],
                    "Bob".into(),
                    vec!["it's really me".to_owned()]
                ),
            ],
            "the reaction is not a line"
        );
        assert!(core.history(bob_chat.clone(), None, 10).unwrap().is_empty());

        // A restart: the requests come back, in the core too.
        drop(core);
        let core = LichenCore::new("t".into(), vec![2; 32], vec![161; 32], data.clone()).unwrap();
        assert_eq!(names(&core).len(), 2);
        assert_eq!(core.lock().core.requests().count(), 2);

        // The real Bob is accepted: his message moves into the conversation, at its time.
        let at = core.requests()[1].messages[0].at_ms;
        assert_eq!(core.accept_request(vec![70; 32]).unwrap(), BOB.to_vec());
        let h = core.history(bob_chat.clone(), None, 10).unwrap();
        assert_eq!(h.len(), 1);
        assert!(matches!(&h[0].message, Message::Text { text, .. } if text == "it's really me"));
        assert_eq!(h[0].at_ms, at);
        assert!(h[0].referable, "its reference uses the accepted identity");
        assert_eq!(names(&core).len(), 1, "Eve's still waits");
        let bob = core
            .contacts()
            .into_iter()
            .find(|c| c.key == BOB.to_vec())
            .unwrap();
        assert_eq!(bob.identity, Some(vec![70; 32]));
        assert!(!bob.verified);

        // Eve is blocked: her identity, listed as a request, not Bob's radio.
        core.block_request(EVE.to_vec()).unwrap();
        assert!(core.requests().is_empty());
        let blocked = core.blocked();
        assert_eq!(blocked.len(), 1);
        assert!(blocked[0].request);
        assert_eq!(blocked[0].name, "Bob");
        assert!(!core.is_blocked(BOB.to_vec()));
        drop(core);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deleting a request forgets it and what it sent.
    #[test]
    fn a_deleted_request_leaves_nothing() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![163; 32], None).unwrap();
        contact(&core);
        {
            let mut i = core.lock();
            i.core.restore_request(BOB, EVE);
            i.on_event(&Event::Requested {
                node: BOB,
                identity: EVE,
            });
            message(&mut i, EVE, text("spam", 1));
        }
        core.delete_request(EVE.to_vec()).unwrap();
        assert!(core.requests().is_empty());
        // A message of hers still queued when the user deleted her is not kept.
        message(&mut core.lock(), EVE, text("more spam", 2));
        assert!(core.requests().is_empty());
        assert!(
            core.lock()
                .store
                .as_ref()
                .unwrap()
                .request_messages(&EVE)
                .unwrap()
                .is_empty()
        );
        assert_eq!(core.lock().core.requests().count(), 0);
        assert!(
            core.history(ChatRef::Contact { key: BOB.to_vec() }, None, 10)
                .unwrap()
                .is_empty()
        );
    }
}

/// Second read of 2026-11-17, N14: at launch, the downloads folder keeps only what a stored
/// message still announces; unfinished `.part` files go, files lichen did not name stay, and
/// nothing goes without this identity's database on disk (the demo's is in memory).
#[cfg(test)]
mod sweep {
    use super::*;
    use lc_core::content::Announce;

    fn announced(core: &LichenCore, hash: [u8; 32]) {
        core.lock().record(store::Message {
            id: 0,
            chat: Chat::Contact([9; 32]),
            outgoing: false,
            author: None,
            content: Content::Announce(Announce {
                sent_at: 1,
                media_id: [0; 8],
                kind: 1,
                hash,
                size: 2_000,
                providers: vec![(1, [5; 32])],
                name: "photo.jpg".into(),
            }),
            at_ms: unix_ms(),
            status: Status::Received,
            fragments: 0,
            airtime_ms: 0,
            snr4: None,
            read: true,
            msg_ref: None,
        });
    }

    #[test]
    fn orphan_downloads_go_at_launch() {
        let root = std::env::temp_dir().join(format!("lichen-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let downloads = root.join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let hex = |b: u8| format!("{b:02x}").repeat(32);
        let kept = format!("{}.jpg", hex(0x66));
        let orphan = format!("{}.png", hex(0x77));
        let part = format!("{}.jpg.part", hex(0x66));
        for name in [
            &kept,
            &orphan,
            &part,
            &"notes.txt".to_owned(),
            &format!("{}.bin", "zz".repeat(32)),
        ] {
            std::fs::write(downloads.join(name), b"x").unwrap();
        }
        let dir = downloads.to_string_lossy().into_owned();

        // In memory (the demo): nothing is touched.
        let demo = LichenCore::new("t".into(), vec![1; 32], vec![171; 32], None).unwrap();
        assert_eq!(demo.sweep_downloads(dir.clone()), 0);

        let data = Some(root.join("data").to_string_lossy().into_owned());
        let core = LichenCore::new("t".into(), vec![1; 32], vec![171; 32], data).unwrap();
        announced(&core, [0x66; 32]);
        assert_eq!(core.sweep_downloads(dir), 2);
        let mut left: Vec<String> = std::fs::read_dir(&downloads)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let mut expected = vec![
            kept,
            "notes.txt".to_owned(),
            format!("{}.bin", "zz".repeat(32)),
        ];
        expected.sort();
        assert_eq!(left, expected);
        drop(core);
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// A request for the full variant of one of our images waits for the user (§13.3).
#[cfg(test)]
mod full_images {
    use super::*;

    const BOB: [u8; 32] = [9; 32];
    const EVE: [u8; 32] = [66; 32];

    fn contact(i: &mut Inner, key: [u8; 32], name: &str) {
        i.core.restore_contact(lc_core::Contact {
            key,
            name: name.into(),
            out_path: None,
            identity: None,
            verified: false,
            kind: lc_core::contact_kind::CHAT,
        });
    }

    /// Our image `media_id` sent to `to`, kept in full as `send_image` keeps it: its row.
    fn sent_image(i: &mut Inner, to: [u8; 32], media_id: [u8; 8]) -> i64 {
        let full = Content::Image {
            sent_at: 1,
            media_id,
            full: true,
            width: 160,
            height: 120,
            full_len: 2_000,
            data: vec![0xA5; 2_000],
        };
        if let Some(st) = &i.store {
            st.put_media(&media_id, &Chat::Contact(to), &full.encode())
                .unwrap();
        }
        i.record(store::Message {
            id: 0,
            chat: Chat::Contact(to),
            outgoing: true,
            author: None,
            content: full,
            at_ms: unix_ms(),
            status: Status::Delivered,
            fragments: 3,
            airtime_ms: 1_000,
            snr4: None,
            read: true,
            msg_ref: None,
        })
        .unwrap()
        .id
    }

    fn asks(core: &LichenCore, from: [u8; 32], media_id: [u8; 8]) -> Vec<LichenEvent> {
        core.lock().on_event(&Event::MediaRequested {
            from,
            media_id,
            full: true,
        });
        std::iter::from_fn(|| core.poll_event())
            .filter(|e| matches!(e, LichenEvent::FullImageRequested { .. }))
            .collect()
    }

    #[test]
    fn a_full_request_waits_for_the_user() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![171; 32], None).unwrap();
        let bob = ChatRef::Contact { key: BOB.to_vec() };
        let row = {
            let mut i = core.lock();
            contact(&mut i, BOB, "Bob");
            contact(&mut i, EVE, "Eve");
            sent_image(&mut i, BOB, [7; 8])
        };
        while core.poll_event().is_some() {}
        // Shown, with what it costs; nothing goes.
        let got = asks(&core, BOB, [7; 8]);
        let [
            LichenEvent::FullImageRequested {
                chat,
                message_id,
                airtime_ms,
            },
        ] = got.as_slice()
        else {
            panic!("{got:?}");
        };
        assert_eq!((chat, *message_id), (&bob, row));
        assert!(*airtime_ms > 1_000, "{airtime_ms} ms for 2 KB");
        let listed = core.full_image_requests(bob.clone());
        assert_eq!(
            listed,
            vec![FullImageRequest {
                message_id: row,
                airtime_ms: *airtime_ms
            }]
        );
        // Asked again while it waits: still one, not shown twice.
        assert!(asks(&core, BOB, [7; 8]).is_empty());
        assert_eq!(core.full_image_requests(bob.clone()).len(), 1);
        // Not Eve's to ask for, nor an image we do not hold: no request.
        assert!(asks(&core, EVE, [7; 8]).is_empty());
        assert!(asks(&core, BOB, [8; 8]).is_empty());
        assert!(
            core.full_image_requests(ChatRef::Contact { key: EVE.to_vec() })
                .is_empty()
        );
        // Declined: forgotten, no answer.
        core.answer_full_image_request(row, false, 0).unwrap();
        assert!(core.full_image_requests(bob.clone()).is_empty());
        assert_eq!(core.answer_full_image_request(row, true, 0), Ok(()));
        // Asking again shows it again.
        assert_eq!(asks(&core, BOB, [7; 8]).len(), 1);
        assert_eq!(core.full_image_requests(bob.clone()).len(), 1);
        // Gone with the image.
        core.delete_for_me(row).unwrap();
        assert!(core.full_image_requests(bob).is_empty());
    }

    #[test]
    fn requests_kept_are_bounded() {
        let core = LichenCore::new("t".into(), vec![1; 32], vec![172; 32], None).unwrap();
        let rows: Vec<i64> = {
            let mut i = core.lock();
            contact(&mut i, BOB, "Bob");
            (0..40u8).map(|n| sent_image(&mut i, BOB, [n; 8])).collect()
        };
        for n in 0..40u8 {
            assert_eq!(asks(&core, BOB, [n; 8]).len(), 1);
        }
        let kept: Vec<i64> = core
            .full_image_requests(ChatRef::Contact { key: BOB.to_vec() })
            .into_iter()
            .map(|r| r.message_id)
            .collect();
        assert_eq!(kept, rows[8..].to_vec());
    }
}
