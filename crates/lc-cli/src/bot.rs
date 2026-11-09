//! What `lichen listen` does besides printing: an echo bot for field tests, received media
//! saved to disk, and an outbox file whose new lines are sent.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use lc_core::content::MediaId;
use lc_core::{Content, Core, Event, GroupId, Millis, PubKey};

use crate::{find_contact, name_of, short, unix_now};

pub struct Bot {
    /// Answer every message (texts are echoed, media summarised).
    pub echo: bool,
    /// Where received images (`.avif`) and voice messages (`.wav`) go.
    pub save: Option<PathBuf>,
    pub outbox: Option<Outbox>,
    /// The last contact who wrote: where outbox lines without `@name` go.
    last_from: Option<PubKey>,
}

/// A file watched for appended lines: `@name text` to a contact, `#group text` to a group,
/// or plain text to the last contact who wrote.
pub struct Outbox {
    path: PathBuf,
    offset: u64,
    pending: String,
    checked: Millis,
}

impl Outbox {
    /// Watches `path` from its current end (created empty if missing).
    pub fn new(path: PathBuf) -> Result<Self, String> {
        if !path.exists() {
            std::fs::write(&path, b"").map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let offset = std::fs::metadata(&path).map_or(0, |m| m.len());
        Ok(Self {
            path,
            offset,
            pending: String::new(),
            checked: 0,
        })
    }

    /// New complete lines since the last call (checked twice a second).
    fn lines(&mut self, now: Millis) -> Vec<String> {
        if now < self.checked + 500 {
            return Vec::new();
        }
        self.checked = now;
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let len = f.metadata().map_or(0, |m| m.len());
        if len < self.offset {
            // Truncated: start again from the top.
            self.offset = 0;
        }
        let mut fresh = String::new();
        if f.seek(SeekFrom::Start(self.offset)).is_err() || f.read_to_string(&mut fresh).is_err() {
            return Vec::new();
        }
        self.offset = len;
        self.pending.push_str(&fresh);
        let mut out = Vec::new();
        while let Some(i) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=i).collect();
            let line = line.trim();
            if !line.is_empty() {
                out.push(line.to_owned());
            }
        }
        out
    }
}

impl Bot {
    pub fn new(echo: bool, save: Option<PathBuf>, outbox: Option<Outbox>) -> Result<Self, String> {
        if let Some(d) = &save {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        Ok(Self {
            echo,
            save,
            outbox,
            last_from: None,
        })
    }

    pub fn on_event(&mut self, core: &mut Core, e: &Event, now: Millis) {
        match e {
            Event::Received {
                from,
                content,
                snr4,
                rssi,
            } => {
                self.last_from = Some(*from);
                let radio = format!("SNR {:.1} dB, RSSI {rssi} dBm", f32::from(*snr4) / 4.0);
                self.keep(core, from, content);
                // A reaction is echoed: the same emoji on the same message (an empty one
                // removes ours). The app never reacts by itself, so this cannot loop.
                if let (true, Content::Reaction { target, emoji, .. }) = (self.echo, content) {
                    let echo = Content::Reaction {
                        sent_at: unix_now(),
                        target: *target,
                        emoji: emoji.clone(),
                    };
                    let what = if emoji.is_empty() {
                        format!("removes its reaction to {}", hex::encode(target))
                    } else {
                        format!("reacts {emoji} to {}", hex::encode(target))
                    };
                    match core.send_datagram(from, echo.encode(), now) {
                        Ok(id) => println!("  -> {} {what} (message {id})", short(from)),
                        Err(e) => println!("  !! reaction not sent: {e:?}"),
                    }
                }
                if let Some(reply) = self.reply(core, from, content, &radio, now) {
                    report(
                        core.send_text(from, &reply, unix_now(), now),
                        core,
                        from,
                        &reply,
                    );
                }
            }
            Event::GroupMessage {
                gid, from, content, ..
            } => {
                self.keep(core, from, content);
                if let (true, Content::Text { text, .. }) = (self.echo, content)
                    && !text.starts_with("echo:")
                {
                    let reply = format!("echo: {text}");
                    match core.send_group_text(gid, &reply, unix_now(), now) {
                        Ok(id) => println!(
                            "  -> [group {}] \"{reply}\" (message {id})",
                            hex::encode(&gid[..4])
                        ),
                        Err(e) => println!("  !! group reply not sent: {e:?}"),
                    }
                }
            }
            _ => {}
        }
    }

    /// The echo bot's answer, if any; asks for the full variant of an image preview.
    fn reply(
        &self,
        core: &mut Core,
        from: &PubKey,
        content: &Content,
        radio: &str,
        now: Millis,
    ) -> Option<String> {
        if !self.echo {
            return None;
        }
        Some(match content {
            Content::Text { text, .. } if text.starts_with("echo:") => return None,
            Content::Text { text, .. } | Content::Reply { text, .. } => {
                format!("echo: {text} · {radio}")
            }
            // A reaction is not answered (it would be an endless exchange); a notice never
            // comes from the air.
            Content::Reaction { .. } | Content::Notice(_) => return None,
            Content::Voice { frames, data, .. } => format!(
                "got your voice message: {:.1} s in {} bytes · {radio}",
                f32::from(*frames) * 0.04,
                data.len()
            ),
            Content::Image {
                media_id,
                full: false,
                full_len,
                width,
                height,
                data,
                ..
            } => {
                if *full_len > 0 {
                    request_full(core, from, media_id, now);
                }
                format!(
                    "got your photo preview, {width}×{height} in {} bytes; {} · {radio}",
                    data.len(),
                    if *full_len > 0 {
                        format!("asking for the full one ({full_len} bytes)")
                    } else {
                        "no full one to ask for".into()
                    }
                )
            }
            Content::Image { data, .. } => {
                format!("got the full photo, {} bytes · {radio}", data.len())
            }
            Content::Location(p) => format!(
                "got your position, ±{:.0} m · {radio}",
                lc_core::location::radius_m(p.prec)
            ),
            Content::Announce(a) => format!(
                "got your announcement of \"{}\" ({} bytes); this CLI has no Internet bridge · {radio}",
                a.name, a.size
            ),
            Content::MediaRequest { .. } | Content::Other { .. } => return None,
        })
    }

    /// Saves received media under `save`.
    fn keep(&self, core: &Core, from: &PubKey, content: &Content) {
        let Some(dir) = &self.save else {
            return;
        };
        let who = name_of(core, from).replace(['/', ' '], "_");
        let stamp = unix_now();
        let (name, bytes) = match content {
            Content::Image {
                media_id,
                full,
                data,
                ..
            } => (
                format!(
                    "{stamp}-{who}-{}-{}.avif",
                    hex::encode(media_id),
                    if *full { "full" } else { "preview" }
                ),
                data.clone(),
            ),
            Content::Voice {
                mode, frames, data, ..
            } => match lc_media::voice::decode(*mode, *frames, data) {
                Some(pcm) => (format!("{stamp}-{who}-voice.wav"), wav(&pcm, 8_000)),
                None => (format!("{stamp}-{who}-voice.c2"), data.clone()),
            },
            _ => return,
        };
        let path = dir.join(name);
        match std::fs::write(&path, bytes) {
            Ok(()) => println!("  saved {}", path.display()),
            Err(e) => println!("  !! {}: {e}", path.display()),
        }
    }

    pub fn on_idle(&mut self, core: &mut Core, now: Millis) {
        let Some(outbox) = &mut self.outbox else {
            return;
        };
        for line in outbox.lines(now) {
            if let Some(rest) = line.strip_prefix('#') {
                let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
                let gid: Option<GroupId> = core
                    .groups()
                    .find(|g| g.name.eq_ignore_ascii_case(name))
                    .map(|g| g.gid);
                match (gid, text.trim()) {
                    (Some(gid), text) if !text.is_empty() => {
                        match core.send_group_text(&gid, text, unix_now(), now) {
                            Ok(id) => println!("  -> [group {name}] \"{text}\" (message {id})"),
                            Err(e) => println!("  !! not sent to group {name}: {e:?}"),
                        }
                    }
                    _ => println!("  !! no group \"{name}\", or nothing to say"),
                }
                continue;
            }
            let (to, text) = match line.strip_prefix('@') {
                Some(rest) => {
                    let (name, text) = rest.split_once(' ').unwrap_or((rest, ""));
                    (find_contact(core, name), text.trim().to_owned())
                }
                None => (
                    self.last_from
                        .ok_or_else(|| "nobody wrote yet: use @name".to_owned()),
                    line.clone(),
                ),
            };
            match to {
                Ok(to) if !text.is_empty() => {
                    report(
                        core.send_text(&to, &text, unix_now(), now),
                        core,
                        &to,
                        &text,
                    );
                }
                Ok(_) => println!("  !! nothing to say"),
                Err(e) => println!("  !! {e}"),
            }
        }
    }
}

fn request_full(core: &mut Core, from: &PubKey, media_id: &MediaId, now: Millis) {
    let request = Content::MediaRequest {
        media_id: *media_id,
        full: true,
    };
    match core.send_datagram(from, request.encode(), now) {
        Ok(id) => println!("  -> full image requested (message {id})"),
        Err(e) => println!("  !! full image not requested: {e:?}"),
    }
}

fn report(r: Result<lc_core::MsgId, lc_core::Error>, core: &Core, to: &PubKey, text: &str) {
    match r {
        Ok(id) => println!("  -> {} \"{text}\" (message {id})", name_of(core, to)),
        Err(e) => println!("  !! not sent to {}: {e:?}", short(to)),
    }
}

/// A mono 16-bit WAV file.
fn wav(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_core::fakenet::Net;
    use lc_meshcore::airtime::LoRaParams;

    /// A reaction to the bot comes back: the same emoji on the same message, and a removal
    /// is mirrored too.
    #[test]
    fn reactions_are_echoed() {
        let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, 7);
        let (phone, bot_node) = (net.add_companion(1, "phone"), net.add_companion(2, "bot"));
        net.link(phone, bot_node);
        net.know(phone, bot_node, true);
        net.know(bot_node, phone, true);
        net.connect(phone);
        net.connect(bot_node);
        net.run_until(10_000, |_| false);
        net.introduce(phone, bot_node);
        let mut bot = Bot::new(true, None, None).unwrap();
        let bot_key = net.key(bot_node);
        let target = [7u8; 8];
        let mut got = Vec::new();
        for emoji in ["👍", ""] {
            let reaction = Content::Reaction {
                sent_at: 1,
                target,
                emoji: emoji.into(),
            };
            net.with_core(phone, |c, now| {
                c.send_datagram(&bot_key, reaction.encode(), now)
            })
            .unwrap()
            .unwrap();
            net.run_until(net.now + 60_000, |n| {
                for e in n.take_events(bot_node) {
                    n.with_core(bot_node, |c, now| bot.on_event(c, &e, now));
                }
                for e in n.take_events(phone) {
                    if let Event::Received {
                        content: Content::Reaction { target, emoji, .. },
                        ..
                    } = e
                    {
                        got.push((target, emoji));
                    }
                }
                false
            });
        }
        assert_eq!(
            got,
            vec![(target, "👍".to_owned()), (target, String::new())]
        );
    }
}
