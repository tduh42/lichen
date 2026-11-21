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

/// Refusals kept until the app reads them.
const REFUSALS_MAX: usize = 16;

impl Core {
    /// The settings the radio refused since the last call, oldest first (§1.1): each one's
    /// value stays the radio's (`RadioSettings` follows), the app says it was not changed.
    pub fn take_radio_refusals(&mut self) -> Vec<RadioSetting> {
        std::mem::take(&mut self.radio_refusals)
    }

    pub(crate) fn radio_refused(&mut self, setting: RadioSetting) {
        if self.radio_refusals.len() < REFUSALS_MAX {
            self.radio_refusals.push(setting);
        }
        self.events.push_back(Event::RadioSettings);
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
        )
    }

    /// Transmit power, within the radio's range.
    pub fn set_tx_power(&mut self, dbm: i8) -> Result<(), Error> {
        if dbm < -9 || dbm > self.settings.max_tx_dbm {
            return Err(Error::Invalid);
        }
        self.radio_set(&Command::SetRadioTxPower { dbm }, RadioSetting::TxPower)
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
        self.radio_set(&Command::SetOtherParams(other), RadioSetting::Preferences)
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
        self.radio_set(&Command::SetCustomVar { name, value }, setting)
    }

    /// Which advert types become contacts by themselves (with manual add), and from how far.
    pub fn set_autoadd(&mut self, mask: u8, max_hops: u8) -> Result<(), Error> {
        self.radio_set(
            &Command::SetAutoaddConfig { mask, max_hops },
            RadioSetting::Autoadd,
        )
    }

    /// The Bluetooth PIN, used from the next boot: 0 for the default, else six digits.
    pub fn set_device_pin(&mut self, pin: u32) -> Result<(), Error> {
        if pin != 0 && !(100_000..=999_999).contains(&pin) {
            return Err(Error::Invalid);
        }
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

    /// Sends a setting, then reads everything again: the settings shown are the radio's.
    fn radio_set(&mut self, cmd: &Command<'_>, setting: RadioSetting) -> Result<(), Error> {
        if !self.online {
            return Err(Error::NoIdentity);
        }
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
    }

    pub(crate) fn radio_autoadd(&mut self, mask: u8, max_hops: u8) {
        self.settings.autoadd = Some((mask, max_hops));
        self.events.push_back(Event::RadioSettings);
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
