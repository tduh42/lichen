//! Problem reports (ADR-0045): what went on lately, for the user to send to the developers,
//! with no message, key, name or position in it.
//!
//! The core's log holds fixed words and numbers only: `Diag::note` takes a `&'static str`, so
//! nothing a person wrote, named or sent can enter it. What the app adds (its own log lines,
//! settings, versions) goes through `mask` before it is written, as a safety net.

use super::*;

/// How many lines the core keeps; the oldest go first.
pub(crate) const DIAG_LINES: usize = 300;

/// One line of the core's log: when (the app's clock, ms), a fixed word, and a number with its
/// unit.
#[derive(Clone, Copy, Debug)]
struct DiagLine {
    at_ms: u64,
    what: &'static str,
    value: Option<(i64, &'static str)>,
}

/// The core's recent log and its counters, in memory only: a report covers the current run.
#[derive(Debug, Default)]
pub(crate) struct Diag {
    lines: VecDeque<DiagLine>,
    frames_in: u64,
    frames_out: u64,
    companion_errors: u64,
}

impl Diag {
    /// Notes `what` at `at_ms`. Only words written in the code can be noted, never a runtime
    /// string.
    pub(crate) fn note(
        &mut self,
        at_ms: u64,
        what: &'static str,
        value: Option<(i64, &'static str)>,
    ) {
        if self.lines.len() >= DIAG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(DiagLine { at_ms, what, value });
    }

    /// A frame from the companion: counted, and an error answer noted with its code.
    pub(crate) fn frame_in(&mut self, at_ms: u64, frame: &[u8]) {
        self.frames_in += 1;
        if let [lc_meshcore::codes::resp::ERR, rest @ ..] = frame {
            self.companion_errors += 1;
            let code = rest.first().map(|&c| (i64::from(c), "code"));
            self.note(at_ms, "companion answered an error", code);
        }
    }

    pub(crate) fn frame_out(&mut self) {
        self.frames_out += 1;
    }

    /// An event of the core, by its kind; a reception with its SNR. Progress is left out (one
    /// per fragment).
    pub(crate) fn event(&mut self, at_ms: u64, e: &Event) {
        let snr = |snr4: i8| Some((i64::from(snr4) / 4, "dB SNR"));
        let (what, value): (&'static str, _) = match e {
            Event::Progress { .. } => return,
            Event::RadioSettings => ("radio settings read", None),
            Event::Connected { .. } => ("radio session started", None),
            Event::ContactsUpdated => ("contacts updated", None),
            Event::IdentityLearned { changed: false, .. } => ("identity learned", None),
            Event::IdentityLearned { changed: true, .. } => ("identity replaced", None),
            Event::IdentityConflict { .. } => ("identity conflict", None),
            Event::SessionEstablished { .. } => ("session established", None),
            Event::Received { snr4, .. } => ("direct message received", snr(*snr4)),
            Event::Delivered { .. } => ("delivered", None),
            Event::Failed { reason, .. } => (fail_word(*reason), None),
            Event::GroupSent { .. } => ("group or channel post sent", None),
            Event::PlainReceived(_) => ("MeshCore message received", None),
            Event::ChannelsUpdated => ("channels updated", None),
            Event::NewerVersion { version, .. } => (
                "a contact speaks a newer protocol",
                Some((i64::from(*version), "version")),
            ),
            Event::GroupInvited { .. } => ("group invitation", None),
            Event::GroupJoined { .. } => ("group joined", None),
            Event::GroupUpdated { .. } => ("group updated", None),
            Event::GroupLeft { .. } => ("group left", None),
            Event::MemberJoined { .. } => ("member joined with an invitation", None),
            Event::GroupChanged { .. } => ("group membership changed", None),
            Event::GroupMessage {
                late: false, snr4, ..
            } => ("group post received", snr(*snr4)),
            Event::GroupMessage { late: true, .. } => ("group post recovered late", None),
            Event::Location { .. } => ("position received", None),
            Event::SharesChanged => ("location shares changed", None),
            Event::MediaRequested { .. } => ("image variant requested", None),
            Event::SearchUpdated { .. } => ("search updated", None),
            Event::Searched { .. } => ("searched by a group member", None),
            Event::Beacon { sos: true, .. } => ("SOS beacon received", None),
            Event::Beacon { sos: false, .. } => ("beacon received", None),
            Event::TxPower { dbm, .. } => ("TX power changed", Some((i64::from(*dbm), "dBm"))),
            Event::Requested { .. } => ("introduction request", None),
            Event::RequestMessage { snr4, .. } => ("request message received", snr(*snr4)),
            Event::RequestAccepted { .. } => ("request accepted", None),
            // Events added later are logged by kind alone until they get a word.
            #[allow(unreachable_patterns)]
            _ => ("other event", None),
        };
        self.note(at_ms, what, value);
    }
}

fn fail_word(reason: CoreFail) -> &'static str {
    match reason {
        CoreFail::NoPath => "failed: no path",
        CoreFail::Unreachable => "failed: no answer to the handshake",
        CoreFail::Timeout => "failed: transfer stalled",
        CoreFail::Interrupted => "failed: interrupted",
        CoreFail::Refused => "failed: refused by the peer",
        CoreFail::TooLarge => "failed: too large for the path",
        CoreFail::IdentityChanged => "failed: identity changed",
        CoreFail::Expired => "failed: expired",
        #[allow(unreachable_patterns)]
        _ => "failed",
    }
}

/// What the app adds to a problem report. Every string goes through the report's mask.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReportApp {
    /// The app's version and build, e.g. "0.9.0 (42)".
    pub app_version: String,
    /// The system, e.g. "iOS 26.1" or "Android 16 (API 36)".
    pub platform: String,
    /// The phone's model, e.g. "iPhone17,3" or "Google Pixel 9".
    pub device: String,
    /// How the radio is reached: "Bluetooth", "TCP", "USB" or "demo".
    pub link: String,
    /// Settings worth knowing, one "name: value" per line.
    pub settings: Vec<String>,
    /// The app's recent log, oldest first.
    pub log: Vec<ReportLine>,
    /// Words that are personal and the core may not know (the name on our contact card, a
    /// Bluetooth device name): masked like contacts' names.
    pub private_words: Vec<String>,
}

/// A line of the app's log, at `at_ms` on the clock the app gives the core (`now_ms`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ReportLine {
    pub at_ms: u64,
    pub text: String,
}

#[uniffi::export]
impl LichenCore {
    /// A problem report (ADR-0045): versions, the radio's model, firmware and settings, a few
    /// counts, and the recent lines of the core's log and of the app's. Never a message, key,
    /// name or position: the core logs fixed words and numbers only, and what the app gives
    /// is masked (names the core knows and `private_words`, runs of 8 or more hex digits,
    /// numbers with 4 or more decimals).
    pub fn problem_report(&self, app: ReportApp, now_ms: u64) -> String {
        let mut i = self.lock();
        let (used, limit) = i.core.budget(now_ms);
        let names = i.private_names(&app.private_words);
        let m = |s: &str| mask(s, &names);
        let offset = i.clock_offset;
        let r = i.core.radio_settings();
        let lora = i.core.radio();
        let mut out = String::new();
        let mut line = |s: String| {
            out.push_str(&s);
            out.push('\n');
        };
        line("lichen problem report".into());
        line(
            "No message, key, name or position is in this report: read it before you send it."
                .into(),
        );
        line(String::new());
        line(format!(
            "When:      {} (the app has run for {})",
            match offset {
                Some(o) => utc_date_time(i64::try_from(now_ms).unwrap_or(0).saturating_add(o)),
                None => "clock not set".into(),
            },
            duration(now_ms),
        ));
        line(format!(
            "Versions:  core {}, protocol {}; app {}; {}; {}",
            env!("CARGO_PKG_VERSION"),
            lc_proto::handshake::VERSION,
            m(&app.app_version),
            m(&app.platform),
            m(&app.device),
        ));
        line(format!(
            "Link:      {}, {}",
            m(&app.link),
            if i.core.ready() && i.core.online() {
                "radio session open"
            } else {
                "no radio session"
            }
        ));
        let known = |s: &str| if s.is_empty() { "?".to_owned() } else { m(s) };
        line(format!(
            "Radio:     {}, firmware {} ({})",
            known(&r.manufacturer),
            known(&r.firmware),
            known(&r.build_date),
        ));
        line(format!(
            "LoRa:      {}.{:03} MHz, SF{}, BW {} kHz, CR 4/{}, TX {} dBm of {}",
            i.core.freq_khz() / 1000,
            i.core.freq_khz() % 1000,
            lora.sf,
            khz(lora.bw_hz),
            lora.cr,
            r.tx_dbm,
            r.max_tx_dbm,
        ));
        if let Some(mv) = r.battery_mv {
            line(format!("Battery:   {mv} mV"));
        }
        line(format!(
            "Companion: {} frames in, {} out, {} error answers",
            i.diag.frames_in, i.diag.frames_out, i.diag.companion_errors
        ));
        line(format!(
            "Airtime:   {}.{} s used of {} s in the last hour",
            used / 1000,
            used % 1000 / 100,
            limit / 1000
        ));
        line(format!(
            "Storage:   {}",
            if i.store.is_some() {
                "open"
            } else {
                "not open"
            }
        ));
        line(format!(
            "Counts:    contacts {}, groups {}, channels {}, texts waiting {}",
            i.core.contacts().filter(|c| c.is_person()).count(),
            i.core.groups().count(),
            i.core.channels().len(),
            i.outbox.len(),
        ));
        for s in &app.settings {
            line(format!("Setting:   {}", m(s)));
        }
        line(String::new());
        line("Core log".into());
        for l in &i.diag.lines {
            let value = l
                .value
                .map_or(String::new(), |(n, unit)| format!(", {n} {unit}"));
            line(format!("{}  {}{value}", clock(l.at_ms, offset), l.what));
        }
        line(String::new());
        line("App log".into());
        for l in &app.log {
            line(format!("{}  {}", clock(l.at_ms, offset), m(&l.text)));
        }
        out
    }
}

impl Inner {
    /// The names a report must not show: contacts (and the names given them here), groups and
    /// their members, invitations, channels, the radio's, and the app's `extra`; longest first,
    /// so that a name inside another is not half masked.
    fn private_names(&self, extra: &[String]) -> Vec<String> {
        let core = &self.core;
        let mut v: Vec<String> = core
            .contacts()
            .map(|c| c.name.clone())
            .chain(self.nicknames.values().cloned())
            .chain(core.groups().flat_map(|g| {
                std::iter::once(g.name.clone()).chain(g.members.iter().map(|m| m.name.clone()))
            }))
            .chain(core.invitations().iter().map(|i| i.name().to_owned()))
            .chain(core.channels().iter().map(|c| c.name.clone()))
            .chain([
                core.node_name().to_owned(),
                core.radio_settings().name.clone(),
            ])
            .chain(extra.iter().cloned())
            .map(|n| n.trim().to_owned())
            .filter(|n| n.chars().count() >= 3)
            .collect();
        v.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        v.dedup();
        v
    }
}

/// `text` with the names in `names` (case ignored, whole words), runs of 8 or more hex digits
/// and numbers with 4 or more decimals replaced by a placeholder.
pub(crate) fn mask(text: &str, names: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let word_start = prev.is_none_or(|p| !p.is_alphanumeric());
        let name = word_start
            .then(|| {
                names.iter().find_map(|n| {
                    let len = starts_with_ci(rest, n)?;
                    let after = rest.get(len..)?.chars().next();
                    after.is_none_or(|a| !a.is_alphanumeric()).then_some(len)
                })
            })
            .flatten();
        let (skip, placeholder) = if let Some(len) = name {
            (len, Some("[name]"))
        } else if let Some(len) = decimals(rest, prev) {
            (len, Some("[number]"))
        } else {
            let hex = rest.bytes().take_while(u8::is_ascii_hexdigit).count();
            if hex >= 8 && prev.is_none_or(|p| !p.is_ascii_hexdigit()) {
                (hex, Some("[hex]"))
            } else {
                (c.len_utf8(), None)
            }
        };
        let Some((taken, after)) = rest.split_at_checked(skip) else {
            break;
        };
        out.push_str(placeholder.unwrap_or(taken));
        prev = taken.chars().last();
        rest = after;
    }
    out
}

/// The length of `needle` at the start of `hay`, comparing without case.
fn starts_with_ci(hay: &str, needle: &str) -> Option<usize> {
    let mut h = hay.char_indices();
    for n in needle.chars() {
        let (_, c) = h.next()?;
        if c != n && !c.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
    }
    Some(h.next().map_or(hay.len(), |(at, _)| at))
}

/// The length of a number with 4 or more decimals at the start of `s` (a coordinate), sign
/// included; `prev` must not be part of a number.
fn decimals(s: &str, prev: Option<char>) -> Option<usize> {
    if prev.is_some_and(|p| p.is_ascii_digit() || p == '.') {
        return None;
    }
    let b = s.as_bytes();
    let sign = usize::from(b.first() == Some(&b'-'));
    let int = b
        .get(sign..)?
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if int == 0 || b.get(sign + int) != Some(&b'.') {
        return None;
    }
    let frac = b
        .get(sign + int + 1..)?
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    (frac >= 4).then_some(sign + int + 1 + frac)
}

/// `HH:MM:SS.mmm` UTC when the wall clock is known, else the time since the app started.
fn clock(at_ms: u64, offset: Option<i64>) -> String {
    match offset {
        Some(o) => {
            let unix = i64::try_from(at_ms).unwrap_or(0).saturating_add(o);
            let ms = unix.rem_euclid(86_400_000);
            format!(
                "{:02}:{:02}:{:02}.{:03}",
                ms / 3_600_000,
                ms / 60_000 % 60,
                ms / 1000 % 60,
                ms % 1000
            )
        }
        None => format!("+{}.{:03}", duration(at_ms), at_ms % 1000),
    }
}

/// `62.5` of 62 500 Hz, `125` of 125 000.
fn khz(hz: u32) -> String {
    match hz % 1000 / 100 {
        0 => format!("{}", hz / 1000),
        tenth => format!("{}.{tenth}", hz / 1000),
    }
}

/// `HH:MM:SS` of `ms`.
fn duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `YYYY-MM-DD HH:MM:SS UTC` of a Unix time in ms (civil from days, proleptic Gregorian).
fn utc_date_time(unix_ms: i64) -> String {
    let days = unix_ms.div_euclid(86_400_000);
    let ms = unix_ms.rem_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DemoNetwork, LichenEvent, Message};

    #[test]
    fn mask_names_keys_and_coordinates() {
        let names = vec!["Camille".to_owned(), "Refuge du Lac".to_owned()];
        assert_eq!(
            mask(
                "camille's radio in Refuge du Lac, key 4F3A9b2c1d0e77",
                &names
            ),
            "[name]'s radio in [name], key [hex]"
        );
        assert_eq!(
            mask("at 45.188530,-5.7245 ok", &names),
            "at [number],[number] ok"
        );
        // Words that only contain a name, short hex and plain numbers stay.
        assert_eq!(
            mask("Camilleta v1.18.0 SF10 869.618 MHz 1234567 deadbee", &names),
            "Camilleta v1.18.0 SF10 869.618 MHz 1234567 deadbee"
        );
        assert_eq!(mask("Ça va, CAMILLE ?", &names), "Ça va, [name] ?");
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc_date_time(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_date_time(1_793_000_000_000), "2026-10-26 07:33:20 UTC");
        assert_eq!(utc_date_time(951_782_400_000), "2000-02-29 00:00:00 UTC");
        assert_eq!(clock(1_500, Some(86_400_000 + 3_600_000)), "01:00:01.500");
        assert_eq!(clock(61_250, None), "+00:01:01.250");
        assert_eq!(
            [khz(62_500), khz(125_000), khz(7_800)],
            ["62.5", "125", "7.8"]
        );
    }

    /// The core's log keeps the last `DIAG_LINES` lines.
    #[test]
    fn the_log_keeps_the_latest_lines() {
        let mut d = Diag::default();
        for at in 0..(DIAG_LINES as u64 + 10) {
            d.note(at, "tick", None);
        }
        assert_eq!(d.lines.len(), DIAG_LINES);
        assert_eq!(d.lines.front().map(|l| l.at_ms), Some(10));
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// A report after messages, a group, positions and names on the demo network carries none
    /// of them, even when the app's own lines do (ADR-0045).
    #[test]
    fn a_report_carries_no_secret() {
        const SECRET: &str = "The spare key is under the blue stone";
        let demo = DemoNetwork::new();
        let core = LichenCore::new("report-test".into(), vec![5; 32], vec![105; 32], None).unwrap();
        core.set_group_consent(false);
        core.set_request_consent(false);
        let wall = 1_793_000_000_000u64;
        core.set_clock(wall, 0);
        core.connect(0);
        let mut secrets: Vec<String> = vec![SECRET.into()];
        let mut sent = false;
        let mut shared = false;
        for step in 0..(4 * 60 * 10) {
            let now = step * 100;
            for f in demo.read(now) {
                core.on_companion_frame(f, now);
            }
            core.set_clock(wall + now, now);
            core.tick(now);
            while let Some(f) = core.poll_companion(now) {
                demo.write(f, now);
            }
            while let Some(e) = core.poll_event() {
                match e {
                    LichenEvent::Received {
                        message: Message::Text { text, .. },
                        ..
                    }
                    | LichenEvent::GroupMessage {
                        message: Message::Text { text, .. },
                        ..
                    } => secrets.push(text),
                    _ => {}
                }
            }
            let camille = core.contacts().into_iter().find(|c| c.name == "Camille");
            if let Some(c) = camille.filter(|_| !sent && now > 30_000) {
                core.send_text(c.key.clone(), SECRET.into(), 0, now)
                    .unwrap();
                core.rename_contact(c.key.clone(), Some("Granny Weatherwax".into()))
                    .unwrap();
                sent = true;
            }
            if !shared && now > 40_000 {
                core.update_location(
                    LocationFix {
                        lat: 45.123_456,
                        lon: 6.654_321,
                        alt_m: None,
                        accuracy_m: 5.0,
                        heading_deg: None,
                        speed_mps: None,
                        unix_s: (wall + now) / 1000,
                    },
                    now,
                );
                if let Some(c) = core
                    .contacts()
                    .into_iter()
                    .find(|c| c.node_name == "Camille")
                {
                    let chat = crate::ChatRef::Contact { key: c.key };
                    core.start_share(chat, crate::SharePrecision::Exact, wall / 1000 + 3600, now)
                        .unwrap();
                    shared = true;
                }
            }
        }
        assert!(sent, "Camille never showed up");
        assert!(secrets.len() > 2, "nothing was received: {secrets:?}");
        let positions = core.positions(4 * 60_000);
        assert!(!positions.is_empty(), "no position received");

        let app_key = core.key().unwrap();
        let app = ReportApp {
            app_version: "0.9.0 (1)".into(),
            platform: "iOS 26.1".into(),
            device: "iPhone17,3".into(),
            link: "Bluetooth".into(),
            settings: vec!["Tor: off".into(), "iroh: on".into()],
            log: vec![
                ReportLine {
                    at_ms: 1_000,
                    text: "Bluetooth: connected to Camille's radio".into(),
                },
                ReportLine {
                    at_ms: 2_000,
                    text: format!("key {} at 45.123456,6.654321 for Morgan", hex(&app_key)),
                },
            ],
            private_words: vec!["Morgan".into()],
        };
        let report = core.problem_report(app, 4 * 60_000);
        eprintln!("{report}");
        assert!(report.contains("lichen problem report"));
        assert!(report.contains("direct message received"));
        assert!(report.contains("Setting:   Tor: off"));
        assert!(report.contains("Bluetooth: connected to [name]'s radio"));
        assert!(report.contains("2026-10-26 07:37:20 UTC"));

        let lower = report.to_lowercase();
        let mut forbidden: Vec<String> = secrets;
        forbidden.extend(["Camille", "Julien", "Granny Weatherwax", "Morgan"].map(String::from));
        for c in core.contacts() {
            forbidden.push(c.name.clone());
            forbidden.push(hex(&c.key));
            forbidden.push(hex(&c.key[..4]));
        }
        for g in core.groups() {
            forbidden.push(g.name.clone());
            forbidden.push(hex(&g.gid));
        }
        for k in [core.key(), core.node_key()].into_iter().flatten() {
            forbidden.push(hex(&k));
            forbidden.push(hex(&k[..4]));
        }
        forbidden.push(core.radio_settings().name);
        for p in &positions {
            forbidden.push(format!("{:.3}", p.lat));
            forbidden.push(format!("{:.3}", p.lon));
        }
        forbidden.extend(["45.123", "6.654"].map(String::from));
        for f in forbidden.iter().filter(|f| f.chars().count() >= 3) {
            assert!(!lower.contains(&f.to_lowercase()), "the report shows {f:?}");
        }
    }
}
