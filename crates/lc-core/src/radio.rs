//! The radio's own settings (protocol §1.1, ADR-0032): read at each connection, set at the
//! user's request, read again after each change. The radio keeps them; nothing is stored here.

use lc_meshcore::{Command, OtherParams, response::DeviceInfo};

use crate::core::{Core, Error, Event, Expect};

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

impl Core {
    /// What the radio reported of its settings.
    pub fn radio_settings(&self) -> &RadioSettings {
        &self.settings
    }

    /// The reads that follow `APP_START` at connection: board settings, auto-add, battery.
    pub(crate) fn radio_read_all(&mut self) {
        self.command(&Command::GetCustomVars, Expect::CustomVars);
        self.command(&Command::GetAutoaddConfig, Expect::Autoadd);
        self.command(&Command::GetBattAndStorage, Expect::Battery);
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
        self.radio_set(&Command::SetRadioParams {
            freq_khz,
            bw_hz,
            sf,
            cr,
            repeat: self.settings.repeat,
        })
    }

    /// Transmit power, within the radio's range.
    pub fn set_tx_power(&mut self, dbm: i8) -> Result<(), Error> {
        if dbm < -9 || dbm > self.settings.max_tx_dbm {
            return Err(Error::Invalid);
        }
        self.radio_set(&Command::SetRadioTxPower { dbm })
    }

    /// The position adverts carry when the policy allows it.
    pub fn set_advert_position(&mut self, lat_e6: i32, lon_e6: i32) -> Result<(), Error> {
        if !(-90_000_000..=90_000_000).contains(&lat_e6)
            || !(-180_000_000..=180_000_000).contains(&lon_e6)
        {
            return Err(Error::Invalid);
        }
        self.radio_set(&Command::SetAdvertLatLon { lat_e6, lon_e6 })
    }

    /// Manual add, telemetry, position in adverts, extra ACKs. The apps ask the user before
    /// a position in adverts or location telemetry (ADR-0032).
    pub fn set_other_params(&mut self, other: OtherParams) -> Result<(), Error> {
        // Each 2-bit telemetry mode is 0, 1 or 2.
        let modes_ok = [0, 2, 4].iter().all(|&s| (other.telemetry >> s) & 3 != 3);
        if other.advert_loc_policy > 1 || other.telemetry > 0x3F || !modes_ok {
            return Err(Error::Invalid);
        }
        self.radio_set(&Command::SetOtherParams(other))
    }

    /// A board setting the radio listed (`gps`: `"1"` / `"0"`, `gps_interval`: seconds).
    pub fn set_radio_var(&mut self, name: &str, value: &str) -> Result<(), Error> {
        if name != "gps_interval" && self.settings.var(name).is_none() {
            return Err(Error::Invalid);
        }
        self.radio_set(&Command::SetCustomVar { name, value })
    }

    /// Which advert types become contacts by themselves (with manual add), and from how far.
    pub fn set_autoadd(&mut self, mask: u8, max_hops: u8) -> Result<(), Error> {
        self.radio_set(&Command::SetAutoaddConfig { mask, max_hops })
    }

    /// The Bluetooth PIN, used from the next boot: 0 for the default, else six digits.
    pub fn set_device_pin(&mut self, pin: u32) -> Result<(), Error> {
        if pin != 0 && !(100_000..=999_999).contains(&pin) {
            return Err(Error::Invalid);
        }
        self.command(&Command::SetDevicePin { pin }, Expect::Ok);
        Ok(())
    }

    /// Restarts the radio: the connection drops and comes back.
    pub fn reboot_radio(&mut self) {
        self.command(&Command::Reboot, Expect::Ok);
    }

    /// Sends a setting, then reads everything again: the settings shown are the radio's.
    fn radio_set(&mut self, cmd: &Command<'_>) -> Result<(), Error> {
        if !self.online {
            return Err(Error::NoIdentity);
        }
        self.command(cmd, Expect::Ok);
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
