//! Unit tests of the binding layer.

use super::*;

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
        let reaction = Event::Received {
            from: [9; 32],
            content: Content::Reaction {
                sent_at: 6,
                target,
                emoji: "👍".into(),
            },
            snr4: 0,
            rssi: 0,
        };
        assert!(!i.on_event(&reaction), "not a message");
        assert!(i.extra.iter().any(|e| matches!(e,
            LichenEvent::Reacted { emoji, target: None, .. } if emoji == "👍")));
        assert!(i.on_event(&Event::Received {
            from: [9; 32],
            content: text,
            snr4: 0,
            rssi: 0,
        }));
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
        let group = |from: u8, content: Content| Event::GroupMessage {
            gid,
            from: [from; 32],
            author: [from + 100; 32],
            content,
            snr4: 0,
            rssi: 0,
            late: false,
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
        let group = |content: Content| Event::GroupMessage {
            gid,
            from: [10; 32],
            author: [110; 32],
            content,
            snr4: 0,
            rssi: 0,
            late: false,
        };
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
        assert!(i.on_event(&Event::GroupMessage {
            gid,
            from: [9; 32],
            author: [7; 32],
            content: text.clone(),
            snr4: 0,
            rssi: 0,
            late: false,
        }));
        let by_identity = message_ref(&[7; 32], &text.encode());
        let st = i.store.as_ref().unwrap();
        assert!(
            st.message_by_ref(&Chat::Group(gid), &by_identity)
                .unwrap()
                .is_some()
        );
        // The author's own reaction, computed on its phone, attaches to it.
        assert!(!i.on_event(&Event::GroupMessage {
            gid,
            from: [9; 32],
            author: [7; 32],
            content: Content::Reaction {
                sent_at: 6,
                target: by_identity,
                emoji: "👍".into(),
            },
            snr4: 0,
            rssi: 0,
            late: false,
        }));
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
        let direct = Event::Received {
            from: [9; 32],
            content: text("hi"),
            snr4: 0,
            rssi: 0,
        };
        assert!(i.on_event(&direct));
        assert!(!i.on_event(&direct));
        let group = |from: u8| Event::GroupMessage {
            late: false,
            gid: [3; 16],
            from: [from; 32],
            author: [from; 32],
            content: text("hi"),
            snr4: 0,
            rssi: 0,
        };
        assert!(i.on_event(&group(1)));
        assert!(!i.on_event(&group(1)));
        // Another member's identical text, or another second, is another message.
        assert!(i.on_event(&group(2)));
        assert!(i.on_event(&Event::Received {
            from: [9; 32],
            content: Content::Text {
                sent_at: 6,
                text: "hi".into()
            },
            snr4: 0,
            rssi: 0,
        }));
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
}
