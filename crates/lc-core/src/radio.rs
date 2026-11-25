//! The radio's own settings (protocol §1.1, ADR-0032): read at each connection, set at the
//! user's request, read again after each change. The radio keeps them; nothing is stored here.

use lc_meshcore::{Command, OtherParams, response::DeviceInfo};

use crate::core::{Core, Error, Event, Expect};
use lc_proto::Millis;

/// How often the battery is read while the radio is connected (ADR-0042).
pub const BATTERY_EVERY_MS: Millis = 10 * 60_000;

/// The battery's charge from its voltage, as the apps show it: 3.3 V empty, 4.2 V full.
pub fn battery_percent(millivolts: u16) -> u8 {
    let pct = (u32::from(millivolts).saturating_sub(3_300) * 100 + 450) / 900;
    u8::try_from(pct.min(100)).unwrap_or(100)
}

/// The radio's figures are read again at most this often when the apps ask (§16.4).
pub const STATS_EVERY_MS: Millis = 60_000;

/// At or below this charge the apps warn that the radio's battery is low (ADR-0042).
pub const BATTERY_LOW_PERCENT: u8 = 15;

/// What lichen knows of the radio's settings. A field the radio has not reported is empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RadioSettings {
    /// The name it advertises.
    pub name: String,
    pub freq_khz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    pub cr: u8,
    pub tx_dbm: i8,
    pub max_tx_dbm: i8,
    /// The position its adverts carry when the policy allows it (degrees × 10⁶); a GPS fix
    /// keeps it current.
    pub lat_e6: i32,
    pub lon_e6: i32,
    pub other: OtherParams,
    /// The board's own settings (`GET_CUSTOM_VARS`), such as `gps`.
    pub vars: Vec<(String, String)>,
    /// Auto-add: advert types mask and maximum hops.
    pub autoadd: Option<(u8, u8)>,
    pub battery_mv: Option<u16>,
    pub storage_used_kb: Option<u32>,
    pub storage_total_kb: Option<u32>,
    pub manufacturer: String,
    pub firmware: String,
    pub build_date: String,
    /// Whether the companion repeats (`SET_RADIO_PARAMS` keeps it as it is).
    pub repeat: bool,
    /// The radio answered (`SELF_INFO`): the values above are its own, not zeros waiting for
    /// it; the last ones read while it is away.
    pub read: bool,
    /// Its figures (`GET_STATS`, §16.4), each unknown until the radio reports it: the noise
    /// floor (dBm), the last packet's RSSI (dBm) and SNR × 4, airtime sent and received (s).
    pub noise_floor_dbm: Option<i16>,
    pub last_rssi: Option<i8>,
    pub last_snr4: Option<i8>,
    pub tx_air_s: Option<u32>,
    pub rx_air_s: Option<u32>,
    /// Seconds since it started; packets received, sent, and received with errors.
    pub uptime_s: Option<u32>,
    pub packets_recv: Option<u32>,
    pub packets_sent: Option<u32>,
    pub recv_errors: Option<u32>,
}

impl RadioSettings {
    /// A board setting's value (`gps` → `"1"`), when the board has it.
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Parses `GET_CUSTOM_VARS`' `name:value,name:value` (ASCII); a malformed pair is skipped.
pub(crate) fn parse_vars(text: &[u8]) -> Vec<(String, String)> {
    text.split(|&b| b == b',')
        .filter_map(|pair| {
            let at = pair.iter().position(|&b| b == b':')?;
            let (name, value) = (pair.get(..at)?, pair.get(at + 1..)?);
            (!name.is_empty()).then(|| {
                (
                    String::from_utf8_lossy(name).into_owned(),
                    String::from_utf8_lossy(value).into_owned(),
                )
            })
        })
        .collect()
}

/// A radio setting the app changes (§1.1), to say which one the radio refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioSetting {
    /// Frequency, bandwidth, spreading factor, coding rate.
    Params,
    TxPower,
    AdvertPosition,
    /// Manual add, telemetry, position in adverts, extra ACKs.
    Preferences,
    Gps,
    GpsInterval,
    Autoadd,
    Pin,
    /// The node's name (§15.4).
    Name,
    /// Another board setting.
    Other,
    /// A contact added from a link (§16.6): the radio did not add it.
    Contact,
}

/// What became of a setting sent to the radio (§1.1, B-user-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingOutcome {
    /// OK, and the radio reports the value asked for.
    Saved,
    /// OK, but the radio still reports another value: it kept its own.
    NotKept,
    /// ERR.
    Refused,
    /// No answer in time, or the radio went away.
    NoAnswer,
}

/// A setting's outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RadioOutcome {
    pub setting: RadioSetting,
    pub outcome: SettingOutcome,
}

/// The value a setting asked for, to compare with what the radio reports back (§1.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    Params {
        freq_khz: u32,
        bw_hz: u32,
        sf: u8,
        cr: u8,
    },
    TxPower(i8),
    AdvertPosition(i32, i32),
    Preferences(OtherParams),
    /// A board setting and its value.
    Var(String, String),
    Autoadd(u8, u8),
    Name(String),
    /// Nothing reports it back (the Bluetooth PIN).
    Unread,
}

/// Which read tells what the radio kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadBack {
    SelfInfo,
    Vars,
    Autoadd,
    None,
}

impl Asked {
    fn read_back(&self) -> ReadBack {
        match self {
            Asked::Params { .. }
            | Asked::TxPower(_)
            | Asked::AdvertPosition(..)
            | Asked::Preferences(_)
            | Asked::Name(_) => ReadBack::SelfInfo,
            Asked::Var(..) => ReadBack::Vars,
            Asked::Autoadd(..) => ReadBack::Autoadd,
            Asked::Unread => ReadBack::None,
        }
    }

    /// Whether the radio's settings hold the value asked for, field by field.
    fn kept(&self, r: &RadioSettings) -> bool {
        match self {
            Asked::Params {
                freq_khz,
                bw_hz,
                sf,
                cr,
            } => (r.freq_khz, r.bw_hz, r.sf, r.cr) == (*freq_khz, *bw_hz, *sf, *cr),
            Asked::TxPower(dbm) => r.tx_dbm == *dbm,
            Asked::AdvertPosition(lat, lon) => (r.lat_e6, r.lon_e6) == (*lat, *lon),
            Asked::Preferences(o) => {
                r.other.manual_add == o.manual_add
                    && r.other.telemetry == o.telemetry
                    && r.other.advert_loc_policy == o.advert_loc_policy
                    && r.other.multi_acks == o.multi_acks
            }
            Asked::Var(name, value) => r.var(name) == Some(value.as_str()),
            Asked::Autoadd(mask, hops) => r.autoadd == Some((*mask, *hops)),
            Asked::Name(name) => r.name == *name,
            Asked::Unread => true,
        }
    }
}

/// A setting sent, waiting for its outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSetting {
    setting: RadioSetting,
    asked: Asked,
    /// The radio answered OK: the read-back decides.
    answered: bool,
}

/// Outcomes kept until the app reads them.
const OUTCOMES_MAX: usize = 16;

impl Core {
    /// What became of the settings sent since the last call, oldest first (§1.1): an
    /// `Event::RadioOutcome` goes with each. The settings shown stay the radio's.
    pub fn take_radio_outcomes(&mut self) -> Vec<RadioOutcome> {
        std::mem::take(&mut self.radio_outcomes)
    }

    /// The settings sent that wait for their outcome, oldest first.
    pub fn radio_pending(&self) -> Vec<RadioSetting> {
        self.radio_pending.iter().map(|p| p.setting).collect()
    }

    pub(crate) fn radio_outcome(&mut self, setting: RadioSetting, outcome: SettingOutcome) {
        let o = RadioOutcome { setting, outcome };
        if self.radio_outcomes.len() < OUTCOMES_MAX {
            self.radio_outcomes.push(o);
        }
        self.events.push_back(Event::RadioOutcome(o));
        self.events.push_back(Event::RadioSettings);
    }

    /// The setting's own command was answered (or not): OK waits for the read-back (one with
    /// nothing to read back is saved), ERR is a refusal, nothing is no answer.
    pub(crate) fn setting_answered(
        &mut self,
        setting: RadioSetting,
        outcome: Option<SettingOutcome>,
    ) {
        let Some(at) = self
            .radio_pending
            .iter()
            .position(|p| p.setting == setting && !p.answered)
        else {
            if let Some(o) = outcome {
                // Not one we track (a contact from a link, §16.6): said as it is.
                self.radio_outcome(setting, o);
            }
            return;
        };
        match outcome {
            None => {
                let unread = self
                    .radio_pending
                    .get(at)
                    .is_some_and(|p| p.asked.read_back() == ReadBack::None);
                if unread {
                    self.radio_pending.remove(at);
                    self.radio_outcome(setting, SettingOutcome::Saved);
                } else if let Some(p) = self.radio_pending.get_mut(at) {
                    p.answered = true;
                }
            }
            Some(o) => {
                self.radio_pending.remove(at);
                self.radio_outcome(setting, o);
            }
        }
    }

    /// A read came back: the settings it tells about, answered OK, are saved or not kept.
    pub(crate) fn settings_read(&mut self, read: ReadBack) {
        let (done, rest): (Vec<PendingSetting>, Vec<PendingSetting>) =
            std::mem::take(&mut self.radio_pending)
                .into_iter()
                .partition(|p| p.answered && p.asked.read_back() == read);
        self.radio_pending = rest;
        for p in done {
            let o = if p.asked.kept(&self.settings) {
                SettingOutcome::Saved
            } else {
                SettingOutcome::NotKept
            };
            self.radio_outcome(p.setting, o);
        }
    }

    /// A read went unanswered, or the radio went away (`None`: everything waiting).
    pub(crate) fn settings_unread(&mut self, read: Option<ReadBack>) {
        let (lost, rest): (Vec<PendingSetting>, Vec<PendingSetting>) =
            std::mem::take(&mut self.radio_pending)
                .into_iter()
                .partition(|p| read.is_none_or(|r| p.answered && p.asked.read_back() == r));
        self.radio_pending = rest;
        for p in lost {
            self.radio_outcome(p.setting, SettingOutcome::NoAnswer);
        }
    }

    /// What the radio reported of its settings.
    pub fn radio_settings(&self) -> &RadioSettings {
        &self.settings
    }

    /// The reads that follow `APP_START` at connection: board settings, auto-add, battery.
    pub(crate) fn radio_read_all(&mut self) {
        self.command(&Command::GetCustomVars, Expect::CustomVars);
        self.command(&Command::GetAutoaddConfig, Expect::Autoadd);
        self.command(&Command::GetBattAndStorage, Expect::Battery);
        self.mesh.battery_at = None;
    }

    /// Reads the battery again when [`BATTERY_EVERY_MS`] passed since it was last read, for the
    /// low-battery warning (ADR-0042). The apps call it as they run (it never wakes them, and
    /// the core's own timing does not depend on it); true when a read went out.
    pub fn radio_refresh_if_due(&mut self, now: Millis) -> bool {
        if !self.online {
            return false;
        }
        match self.mesh.battery_at {
            None => {
                self.mesh.battery_at = Some(now);
                false
            }
            Some(t) if now.saturating_sub(t) >= BATTERY_EVERY_MS => {
                self.mesh.battery_at = Some(now);
                self.command(&Command::GetBattAndStorage, Expect::Battery);
                true
            }
            Some(_) => false,
        }
    }

    /// Reads the radio's figures again (noise floor, airtime, uptime, packets: §16.4) when
    /// [`STATS_EVERY_MS`] passed since they were last read; the apps call it while Radio
    /// settings is open. True when a read went out.
    pub fn radio_refresh_stats(&mut self, now: Millis) -> bool {
        if !self.online
            || self
                .mesh
                .stats_at
                .is_some_and(|t| now.saturating_sub(t) < STATS_EVERY_MS)
        {
            return false;
        }
        self.radio_read_stats(now);
        true
    }

    /// Asks for the radio's figures, all three types of `GET_STATS` (§16.4).
    pub(crate) fn radio_read_stats(&mut self, now: Millis) {
        for kind in [0, 1, 2] {
            self.command(&Command::GetStats { kind }, Expect::Stats);
        }
        self.mesh.stats_at = Some(now);
    }

    /// `GET_STATS` type 0: uptime, and the battery read again.
    pub(crate) fn radio_core_stats(&mut self, battery_mv: u16, uptime_s: u32) {
        self.settings.uptime_s = Some(uptime_s);
        if battery_mv > 0 {
            self.settings.battery_mv = Some(battery_mv);
        }
        self.events.push_back(Event::RadioSettings);
    }

    /// `GET_STATS` type 1: a noise floor of 0 (not sampled yet) and an RSSI of 0 (none, or a
    /// radio that does not measure it) are unknown.
    pub(crate) fn radio_radio_stats(&mut self, noise: i16, rssi: i8, snr4: i8, tx: u32, rx: u32) {
        self.settings.noise_floor_dbm = (noise != 0).then_some(noise);
        self.settings.last_rssi = (rssi != 0).then_some(rssi);
        self.settings.last_snr4 = Some(snr4);
        self.settings.tx_air_s = Some(tx);
        self.settings.rx_air_s = Some(rx);
        self.events.push_back(Event::RadioSettings);
    }

    /// `GET_STATS` type 2: packets received, sent, received with errors.
    pub(crate) fn radio_packet_stats(&mut self, recv: u32, sent: u32, errors: u32) {
        self.settings.packets_recv = Some(recv);
        self.settings.packets_sent = Some(sent);
        self.settings.recv_errors = Some(errors);
        self.events.push_back(Event::RadioSettings);
    }

    /// Reads the battery and storage again (the radio card); nothing before the connection,
    /// which reads them anyway.
    pub fn radio_refresh(&mut self) {
        if self.online {
            self.command(&Command::GetBattAndStorage, Expect::Battery);
        }
    }

    /// LoRa parameters: frequency (kHz), bandwidth (Hz), spreading factor, coding rate. The
    /// radio applies them at once; a radio on other settings no longer hears its mesh.
    pub fn set_radio_params(
        &mut self,
        freq_khz: u32,
        bw_hz: u32,
        sf: u8,
        cr: u8,
    ) -> Result<(), Error> {
        if !(150_000..=2_500_000).contains(&freq_khz)
            || !(7_000..=500_000).contains(&bw_hz)
            || !(5..=12).contains(&sf)
            || !(5..=8).contains(&cr)
        {
            return Err(Error::Invalid);
        }
        self.radio_set(
            &Command::SetRadioParams {
                freq_khz,
                bw_hz,
                sf,
                cr,
                repeat: self.settings.repeat,
            },
            RadioSetting::Params,
            Asked::Params {
                freq_khz,
                bw_hz,
                sf,
                cr,
            },
        )
    }

    /// Transmit power, within the radio's range.
    pub fn set_tx_power(&mut self, dbm: i8) -> Result<(), Error> {
        if dbm < -9 || dbm > self.settings.max_tx_dbm {
            return Err(Error::Invalid);
        }
        self.radio_set(
            &Command::SetRadioTxPower { dbm },
            RadioSetting::TxPower,
            Asked::TxPower(dbm),
        )
    }

    /// The position adverts carry when the policy allows it.
    pub fn set_advert_position(&mut self, lat_e6: i32, lon_e6: i32) -> Result<(), Error> {
        if !(-90_000_000..=90_000_000).contains(&lat_e6)
            || !(-180_000_000..=180_000_000).contains(&lon_e6)
        {
            return Err(Error::Invalid);
        }
        self.radio_set(
            &Command::SetAdvertLatLon { lat_e6, lon_e6 },
            RadioSetting::AdvertPosition,
            Asked::AdvertPosition(lat_e6, lon_e6),
        )
    }

    /// Manual add, telemetry, position in adverts, extra ACKs. The apps ask the user before
    /// a position in adverts or location telemetry (ADR-0032).
    pub fn set_other_params(&mut self, other: OtherParams) -> Result<(), Error> {
        // Each 2-bit telemetry mode is 0, 1 or 2.
        let modes_ok = [0, 2, 4].iter().all(|&s| (other.telemetry >> s) & 3 != 3);
        if other.advert_loc_policy > 1 || other.telemetry > 0x3F || !modes_ok {
            return Err(Error::Invalid);
        }
        self.radio_set(
            &Command::SetOtherParams(other),
            RadioSetting::Preferences,
            Asked::Preferences(other),
        )
    }

    /// A board setting the radio listed (`gps`: `"1"` / `"0"`, `gps_interval`: seconds).
    pub fn set_radio_var(&mut self, name: &str, value: &str) -> Result<(), Error> {
        if name != "gps_interval" && self.settings.var(name).is_none() {
            return Err(Error::Invalid);
        }
        let setting = match name {
            "gps" => RadioSetting::Gps,
            "gps_interval" => RadioSetting::GpsInterval,
            _ => RadioSetting::Other,
        };
        self.radio_set(
            &Command::SetCustomVar { name, value },
            setting,
            Asked::Var(name.to_owned(), value.to_owned()),
        )
    }

    /// Which advert types become contacts by themselves (with manual add), and from how far.
    pub fn set_autoadd(&mut self, mask: u8, max_hops: u8) -> Result<(), Error> {
        self.radio_set(
            &Command::SetAutoaddConfig { mask, max_hops },
            RadioSetting::Autoadd,
            Asked::Autoadd(mask, max_hops),
        )
    }

    /// The Bluetooth PIN, used from the next boot: 0 for the default, else six digits.
    pub fn set_device_pin(&mut self, pin: u32) -> Result<(), Error> {
        if pin != 0 && !(100_000..=999_999).contains(&pin) {
            return Err(Error::Invalid);
        }
        if !self.online {
            return Err(Error::NoIdentity);
        }
        self.radio_pending.push(PendingSetting {
            setting: RadioSetting::Pin,
            asked: Asked::Unread,
            answered: false,
        });
        self.command(
            &Command::SetDevicePin { pin },
            Expect::Setting(RadioSetting::Pin),
        );
        Ok(())
    }

    /// Restarts the radio: the connection drops and comes back.
    pub fn reboot_radio(&mut self) {
        self.command(&Command::Reboot, Expect::Ok);
    }

    /// Sends a setting, then reads everything again: the settings shown are the radio's, and
    /// the read-back tells whether it kept the value `asked` (§1.1).
    fn radio_set(
        &mut self,
        cmd: &Command<'_>,
        setting: RadioSetting,
        asked: Asked,
    ) -> Result<(), Error> {
        if !self.online {
            return Err(Error::NoIdentity);
        }
        self.radio_pending.push(PendingSetting {
            setting,
            asked,
            answered: false,
        });
        self.command(cmd, Expect::Setting(setting));
        let name = self.app_name.clone();
        self.command(
            &Command::AppStart {
                name: name.as_bytes(),
            },
            Expect::SelfInfo,
        );
        self.radio_read_all();
        Ok(())
    }

    pub(crate) fn radio_device_info(&mut self, d: &DeviceInfo<'_>) {
        let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_owned();
        self.settings.manufacturer = text(d.manufacturer);
        self.settings.firmware = text(d.version);
        self.settings.build_date = text(d.build_date);
        self.settings.repeat = d.repeat.unwrap_or(false);
        self.events.push_back(Event::RadioSettings);
    }

    pub(crate) fn radio_vars(&mut self, text: &[u8]) {
        self.settings.vars = parse_vars(text);
        self.events.push_back(Event::RadioSettings);
        self.settings_read(ReadBack::Vars);
    }

    pub(crate) fn radio_autoadd(&mut self, mask: u8, max_hops: u8) {
        self.settings.autoadd = Some((mask, max_hops));
        self.events.push_back(Event::RadioSettings);
        self.settings_read(ReadBack::Autoadd);
    }

    /// Our node's new name (§15.4): sent, then read back with the rest (§1.1).
    pub(crate) fn rename_radio(&mut self, name: &str) {
        self.radio_pending.push(PendingSetting {
            setting: RadioSetting::Name,
            asked: Asked::Name(name.to_owned()),
            answered: false,
        });
        self.command(
            &Command::SetAdvertName {
                name: name.as_bytes(),
            },
            Expect::Setting(RadioSetting::Name),
        );
    }

    /// The read-back after a rename (queued after its advert).
    pub(crate) fn read_back_name(&mut self) {
        let app = self.app_name.clone();
        self.command(
            &Command::AppStart {
                name: app.as_bytes(),
            },
            Expect::SelfInfo,
        );
    }

    pub(crate) fn radio_battery(
        &mut self,
        millivolts: u16,
        used_kb: Option<u32>,
        total_kb: Option<u32>,
    ) {
        self.settings.battery_mv = Some(millivolts);
        self.settings.storage_used_kb = used_kb;
        self.settings.storage_total_kb = total_kb;
        self.events.push_back(Event::RadioSettings);
    }
}
