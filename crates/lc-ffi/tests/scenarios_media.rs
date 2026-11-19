//! Media over LoRa (docs/testing.md §6, MD-1 to MD-6) with Camille. MD-7 (the bridge) needs
//! the Internet: `cargo test -p lc-ffi --release -- --ignored bridge`.

mod common;

use common::{Sim, contact_chat, text};
use lc_ffi::{
    ChatRef, LichenError, LichenEvent, Message, MessageStatus, PreparedImage, Provider,
    StoredMessage, decode_voice, gif_link, image_datagram_len, prepare_image, prepare_voice,
    voice_datagram_len,
};

fn photo(w: u32, h: u32) -> PreparedImage {
    prepare_image(lc_media::image::sample_landscape(w, h), w, h).unwrap()
}

/// Waits until message `id` of `chat` is delivered (or fails the test).
fn delivered(sim: &mut Sim, chat: &ChatRef, id: i64) -> StoredMessage {
    let m = sim.until_status(chat, id, 600, |s| {
        matches!(s, MessageStatus::Delivered | MessageStatus::Failed { .. })
    });
    assert_eq!(m.status, MessageStatus::Delivered, "{m:?}");
    m
}

/// MD-1: a photo's cost is known before it goes; its preview is delivered and the history
/// keeps the full image; photos beyond the hour's airtime are refused, not queued.
#[test]
fn md_1_send_a_photo() {
    let mut sim = Sim::new(131);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let img = photo(640, 480);
    assert!(img.preview.len() <= 450, "{}", img.preview.len());
    assert!(img.full.len() > img.preview.len());
    let len = image_datagram_len(&img);
    let cost = sim
        .core
        .estimate_datagram(camille.key.clone(), len)
        .unwrap();
    assert!(cost.fragments >= 1 && cost.airtime_ms > 0, "{cost:?}");
    let m = sim
        .core
        .send_image(camille.key.clone(), img.clone(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(
        matches!(m.message, Message::Image { full: true, .. }),
        "ours kept in full"
    );
    let m = delivered(&mut sim, &chat, m.id);
    assert_eq!(m.fragments, cost.fragments);
    // Over the hour's airtime: refused, with the wait, and nothing kept.
    let before = sim.history(&chat).len();
    let mut sent = 0;
    let wait = loop {
        match sim
            .core
            .send_image(camille.key.clone(), img.clone(), sim.unix_s(), sim.now)
        {
            Ok(_) => sent += 1,
            Err(LichenError::OverBudget { wait_s }) => break wait_s,
            Err(e) => panic!("{e:?}"),
        }
        assert!(sent < 1_000, "never refused");
    };
    assert!(wait > 0);
    assert_eq!(sim.history(&chat).len(), before + sent);
    let b = sim.core.budget(sim.now);
    assert!(b.limit_ms > 0, "{b:?}");
    // A picture that is not one.
    assert_eq!(
        prepare_image(vec![1, 2, 3], 10, 10).err(),
        Some(LichenError::BadImage)
    );
}

/// MD-2: Camille asks for our photo in full: the request shows on it; Not now sends nothing
/// and clears it; she asks again and it shows again; Send gets it to her.
#[test]
fn md_2_full_photo_requested() {
    let mut sim = Sim::new(132);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    // Once her own photo has gone: the hub is busy before.
    sim.run_to(300_000);
    let m = sim
        .core
        .send_image(camille.key.clone(), photo(320, 240), sim.unix_s(), sim.now)
        .unwrap();
    delivered(&mut sim, &chat, m.id);
    let (c, id, airtime) = sim.until_event("her request", 120, |e| match e {
        LichenEvent::FullImageRequested {
            chat,
            message_id,
            airtime_ms,
        } => Some((chat.clone(), *message_id, *airtime_ms)),
        _ => None,
    });
    assert_eq!((c, id), (chat.clone(), m.id));
    assert!(airtime > 0);
    let waiting = sim.core.full_image_requests(chat.clone());
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].message_id, m.id);
    // Not now: nothing goes, the card goes.
    sim.core
        .answer_full_image_request(m.id, false, sim.now)
        .unwrap();
    assert!(sim.core.full_image_requests(chat.clone()).is_empty());
    // She asks once more, three minutes after.
    sim.until_event("her second request", 240, |e| match e {
        LichenEvent::FullImageRequested { message_id, .. } if *message_id == m.id => Some(()),
        _ => None,
    });
    assert_eq!(sim.core.full_image_requests(chat.clone()).len(), 1);
    sim.core
        .answer_full_image_request(m.id, true, sim.now)
        .unwrap();
    assert!(sim.core.full_image_requests(chat.clone()).is_empty());
    sim.until("her thanks", 600, |c| {
        common::texts(&c.history(chat.clone(), None, 50).unwrap())
            .contains(&"Got the full picture, thanks!".to_owned())
    });
    // A second tap on Send, the request gone: nothing more.
    sim.core
        .answer_full_image_request(m.id, true, sim.now)
        .unwrap();
    assert!(sim.core.full_image_requests(chat).is_empty());
}

/// MD-3: Camille's photo: a preview with its caption, listed with the chat's media; asked
/// for, the same message becomes the full image.
#[test]
fn md_3_receive_a_photo() {
    let mut sim = Sim::new(133);
    let camille = sim.contact("Camille");
    let chat = contact_chat(&camille);
    let preview = sim.until_some("her photo", 400, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| matches!(m.message, Message::Image { full: false, .. }))
    });
    let Message::Image {
        width,
        height,
        full_len,
        data,
        ..
    } = &preview.message
    else {
        unreachable!()
    };
    assert!(*width > 0 && *height > 0 && *full_len > 0 && !data.is_empty());
    assert!(!preview.outgoing);
    let h = sim.history(&chat);
    let at = h.iter().position(|m| m.id == preview.id).unwrap();
    assert_eq!(
        text(&h[at - 1].message),
        Some("The view from the refuge tonight:"),
        "the caption just before"
    );
    let media = sim.core.chat_media(chat.clone(), None, 10).unwrap();
    assert!(media.iter().any(|m| m.id == preview.id));
    // Not a preview: refused.
    assert_eq!(
        sim.core.request_full_image(h[at - 1].id, sim.now),
        Err(LichenError::NotAPreview)
    );
    sim.core.request_full_image(preview.id, sim.now).unwrap();
    let full = sim.until_some("the full photo", 600, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.id == preview.id && matches!(m.message, Message::Image { full: true, .. }))
    });
    let Message::Image { data, .. } = &full.message else {
        unreachable!()
    };
    assert_eq!(data.len(), usize::from(*full_len));
    assert_eq!(
        sim.history(&chat)
            .iter()
            .filter(|m| matches!(m.message, Message::Image { .. }))
            .count(),
        1,
        "the same message, not a second one"
    );
}

/// MD-4: a voice note recorded, reviewed (its cost), sent and delivered; Camille answers with
/// hers; both play.
#[test]
fn md_4_voice_record_send_play() {
    let mut sim = Sim::new(134);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let pcm: Vec<i16> = (0..24_000).map(|i| ((i % 80) as i16 - 40) * 200).collect();
    let v = prepare_voice(pcm, 0);
    assert_eq!((v.mode, v.frames, v.duration_ms), (0, 75, 3_000));
    let cost = sim
        .core
        .estimate_datagram(camille.key.clone(), voice_datagram_len(&v))
        .unwrap();
    assert!(cost.airtime_ms > 0);
    let m = sim
        .core
        .send_voice(camille.key.clone(), v.clone(), sim.unix_s(), sim.now)
        .unwrap();
    delivered(&mut sim, &chat, m.id);
    let ours = sim.message(&chat, m.id);
    let Message::Voice {
        mode, frames, data, ..
    } = ours.message
    else {
        panic!("{ours:?}")
    };
    assert_eq!(
        decode_voice(mode, frames, data).unwrap().len(),
        usize::from(frames) * 320
    );
    let hers = sim.until_some("her voice note", 300, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| !m.outgoing && matches!(m.message, Message::Voice { .. }))
    });
    let Message::Voice {
        mode,
        frames,
        data,
        duration_ms,
        ..
    } = hers.message
    else {
        unreachable!()
    };
    assert!(duration_ms > 0);
    assert_eq!(
        decode_voice(mode, frames, data).unwrap().len(),
        usize::from(frames) * 320
    );
    // Silence or nothing recorded: nothing to send.
    let empty = prepare_voice(vec![], 0);
    assert!(
        sim.core
            .send_voice(camille.key.clone(), empty, sim.unix_s(), sim.now)
            .is_err()
    );
}

/// MD-5: a small note goes over LoRa, compressed; a big document does not fit, and is offered
/// over the bridge instead (an announcement).
#[test]
fn md_5_files_small_note_and_big_document() {
    let mut sim = Sim::new(135);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let note: Vec<u8> = "Water at the spring, keys under the stone by the door.\n"
        .repeat(55)
        .into_bytes();
    assert!(note.len() > 3_000);
    let lora = sim
        .core
        .lora_file(camille.key.clone(), "refuge-notes.txt".into(), note.clone())
        .unwrap()
        .expect("fits over LoRa");
    assert!(lora.compressed && (lora.wire_bytes as usize) < note.len());
    assert!(lora.airtime_ms > 0);
    let m = sim
        .core
        .send_file(
            camille.key.clone(),
            "refuge-notes.txt".into(),
            note.clone(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let m = delivered(&mut sim, &chat, m.id);
    assert!(matches!(&m.message, Message::File { name, data, .. }
        if name == "refuge-notes.txt" && *data == note));
    // 240 KB: not over LoRa.
    let big: Vec<u8> = (0..240_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    assert_eq!(
        sim.core
            .lora_file(camille.key.clone(), "map.pdf".into(), big.clone())
            .unwrap(),
        None
    );
    assert_eq!(
        sim.core
            .send_file(
                camille.key.clone(),
                "map.pdf".into(),
                big,
                sim.unix_s(),
                sim.now
            )
            .err(),
        Some(LichenError::TooLarge)
    );
    // Offered over the bridge: an announcement, one small frame.
    let a = sim
        .core
        .send_announce(
            chat.clone(),
            vec![7; 8],
            3,
            vec![9; 32],
            240_000,
            vec![Provider {
                transport: 1,
                key: vec![5; 32],
            }],
            "map.pdf".into(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let a = delivered(&mut sim, &chat, a.id);
    assert!(
        matches!(&a.message, Message::Announce { name, size, kind: 3, .. }
        if name == "map.pdf" && *size == 240_000)
    );
    assert!(
        sim.core
            .chat_media(chat, None, 10)
            .unwrap()
            .iter()
            .any(|x| x.id == a.id)
    );
}

/// MD-6: a sticker, answered with one of Camille's; a GIPHY link sent as a GIF.
#[test]
fn md_6_gifs_and_stickers() {
    let mut sim = Sim::new(136);
    let camille = sim.camille();
    let chat = contact_chat(&camille);
    let s = sim
        .core
        .send_sticker(chat.clone(), "👍".into(), sim.unix_s(), sim.now)
        .unwrap();
    assert!(matches!(&s.message, Message::Sticker { emoji, .. } if emoji == "👍"));
    delivered(&mut sim, &chat, s.id);
    let hers = sim.until_some("her sticker", 300, |c| {
        c.history(chat.clone(), None, 50)
            .unwrap()
            .into_iter()
            .find(|m| !m.outgoing && matches!(m.message, Message::Sticker { .. }))
    });
    assert!(matches!(&hers.message, Message::Sticker { emoji, .. } if emoji == "🥳"));
    let link = gif_link("https://giphy.com/gifs/sunset-sunrise-denali-l4KidtNiqsus526M8".into())
        .expect("a GIPHY link");
    assert_eq!(link.id, "l4KidtNiqsus526M8");
    let g = sim
        .core
        .send_gif(
            chat.clone(),
            link.id.clone(),
            200,
            102,
            link.title.clone(),
            sim.unix_s(),
            sim.now,
        )
        .unwrap();
    let g = delivered(&mut sim, &chat, g.id);
    assert!(matches!(&g.message, Message::Gif { id, .. } if *id == link.id));
    assert_eq!(
        gif_link("look at this https://giphy.com/gifs/x-abc".into()),
        None
    );
    assert!(
        sim.core
            .send_sticker(chat, "👍".repeat(20), sim.unix_s(), sim.now)
            .is_err()
    );
}
