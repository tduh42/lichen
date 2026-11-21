//! The radio's own settings (protocol §1.1, ADR-0032): read at each connection, set at the
//! user's request; the radio keeps them.

use super::*;
use lc_meshcore::OtherParams;

/// A board setting the radio lists (`gps` = `"1"`).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RadioVar {
    pub name: String,
    pub value: String,
}

/// Who may ask the radio for telemetry, per kind: 0 nobody, 1 the contacts allowed one by one,
/// 2 everyone.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Telemetry {
    pub base: u8,
    pub location: u8,
    pub environment: u8,
}

/// What the radio reported of its settings; an empty field was not reported.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RadioSettings {
    pub name: String,
    pub freq_khz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    pub cr: u8,
    pub tx_dbm: i8,
    pub max_tx_dbm: i8,
    /// The position adverts carry when `position_in_adverts` is on (degrees × 10⁶).
    pub lat_e6: i32,
    pub lon_e6: i32,
    /// Adverts carry the radio's position, in clear, for the whole mesh.
    pub position_in_adverts: bool,
    pub telemetry: Telemetry,
    /// Only the advert types of `autoadd_mask` become contacts by themselves.
    pub manual_add: bool,
    /// 0x01 overwrite the oldest when full, 0x02 chat, 0x04 repeater, 0x08 room, 0x10 sensor.
    pub autoadd_mask: Option<u8>,
    pub autoadd_max_hops: Option<u8>,
    pub multi_acks: u8,
    /// `Some` when the board has a GPS: whether it is on.
    pub gps: Option<bool>,
    /// Seconds between GPS fixes, when the board reports it.
    pub gps_interval_s: Option<u32>,
    pub vars: Vec<RadioVar>,
    pub battery_mv: Option<u16>,
    pub storage_used_kb: Option<u32>,
    pub storage_total_kb: Option<u32>,
    pub manufacturer: String,
    pub firmware: String,
    pub build_date: String,
    /// The battery's charge (3.3 V empty, 4.2 V full), and whether it is low enough to warn
    /// (ADR-0042); read again at most every 10 minutes while connected.
    pub battery_percent: Option<u8>,
    pub battery_low: bool,
    /// The radio answered: the values are its own. False until then (they are zeros): the
    /// apps say the settings are being read (QA bug B-ios-15).
    pub read: bool,
}

impl From<&lc_core::radio::RadioSettings> for RadioSettings {
    fn from(r: &lc_core::radio::RadioSettings) -> Self {
        let t = r.other.telemetry;
        Self {
            name: r.name.clone(),
            freq_khz: r.freq_khz,
            bw_hz: r.bw_hz,
            sf: r.sf,
            cr: r.cr,
            tx_dbm: r.tx_dbm,
            max_tx_dbm: r.max_tx_dbm,
            lat_e6: r.lat_e6,
            lon_e6: r.lon_e6,
            position_in_adverts: r.other.advert_loc_policy != 0,
            telemetry: Telemetry {
                base: t & 3,
                location: (t >> 2) & 3,
                environment: (t >> 4) & 3,
            },
            manual_add: r.other.manual_add,
            autoadd_mask: r.autoadd.map(|(m, _)| m),
            autoadd_max_hops: r.autoadd.map(|(_, h)| h),
            multi_acks: r.other.multi_acks,
            gps: r.var("gps").map(|v| v == "1"),
            gps_interval_s: r.var("gps_interval").and_then(|v| v.parse().ok()),
            vars: r
                .vars
                .iter()
                .map(|(name, value)| RadioVar {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            battery_mv: r.battery_mv,
            storage_used_kb: r.storage_used_kb,
            storage_total_kb: r.storage_total_kb,
            manufacturer: r.manufacturer.clone(),
            firmware: r.firmware.clone(),
            build_date: r.build_date.clone(),
            battery_percent: r.battery_mv.map(lc_core::radio::battery_percent),
            battery_low: r.battery_mv.is_some_and(|mv| {
                lc_core::radio::battery_percent(mv) <= lc_core::radio::BATTERY_LOW_PERCENT
            }),
            read: r.read,
        }
    }
}

/// A radio setting the app changes (§1.1), to say which one the radio refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
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
    Other,
    /// A contact added from a link (§16.6): the radio did not add it (its table is full, or
    /// it refused the record).
    Contact,
}

impl From<lc_core::radio::RadioSetting> for RadioSetting {
    fn from(s: lc_core::radio::RadioSetting) -> Self {
        use lc_core::radio::RadioSetting as S;
        match s {
            S::Params => Self::Params,
            S::TxPower => Self::TxPower,
            S::AdvertPosition => Self::AdvertPosition,
            S::Preferences => Self::Preferences,
            S::Gps => Self::Gps,
            S::GpsInterval => Self::GpsInterval,
            S::Autoadd => Self::Autoadd,
            S::Pin => Self::Pin,
            S::Name => Self::Name,
            S::Other => Self::Other,
            S::Contact => Self::Contact,
        }
    }
}

#[uniffi::export]
impl LichenCore {
    /// The settings the radio itself refused (it answered ERR) since the last call, oldest
    /// first: a `RadioSettings` event follows each, the value shown stays the radio's, and the
    /// app says the setting was not changed (§1.1, QA bug B-ios-10).
    pub fn take_radio_refusals(&self) -> Vec<RadioSetting> {
        self.lock()
            .core
            .take_radio_refusals()
            .into_iter()
            .map(RadioSetting::from)
            .collect()
    }

    /// What the radio reported of its settings (a `RadioSettings` event follows each change).
    pub fn radio_settings(&self) -> RadioSettings {
        RadioSettings::from(self.lock().core.radio_settings())
    }

    /// Reads the battery and storage again.
    pub fn refresh_radio(&self) {
        self.lock().core.radio_refresh();
    }

    /// Reads the battery again when 10 minutes passed since it was last read (the low-battery
    /// warning, ADR-0042): call it as the app runs, with `pump`.
    pub fn refresh_battery_if_due(&self, now_ms: u64) {
        self.lock().core.radio_refresh_if_due(now_ms);
    }

    /// LoRa parameters, applied by the radio at once: a radio on other settings no longer
    /// hears its mesh, so the app asks first.
    pub fn set_radio_params(
        &self,
        freq_khz: u32,
        bw_hz: u32,
        sf: u8,
        cr: u8,
    ) -> Result<(), LichenError> {
        Ok(self.lock().core.set_radio_params(freq_khz, bw_hz, sf, cr)?)
    }

    pub fn set_tx_power(&self, dbm: i8) -> Result<(), LichenError> {
        Ok(self.lock().core.set_tx_power(dbm)?)
    }

    /// The position adverts carry when `position_in_adverts` is on.
    pub fn set_advert_position(&self, lat_e6: i32, lon_e6: i32) -> Result<(), LichenError> {
        Ok(self.lock().core.set_advert_position(lat_e6, lon_e6)?)
    }

    /// Manual add, telemetry, position in adverts, extra ACKs, all at once. The app asks the
    /// user before turning on a position in adverts or location telemetry (ADR-0032).
    pub fn set_radio_preferences(
        &self,
        manual_add: bool,
        telemetry: Telemetry,
        position_in_adverts: bool,
        multi_acks: u8,
    ) -> Result<(), LichenError> {
        if telemetry.base > 2 || telemetry.location > 2 || telemetry.environment > 2 {
            return Err(LichenError::InvalidSetting);
        }
        Ok(self.lock().core.set_other_params(OtherParams {
            manual_add,
            telemetry: telemetry.environment << 4 | telemetry.location << 2 | telemetry.base,
            advert_loc_policy: u8::from(position_in_adverts),
            multi_acks,
        })?)
    }

    /// The board's GPS on or off (when it has one).
    pub fn set_gps(&self, on: bool) -> Result<(), LichenError> {
        Ok(self
            .lock()
            .core
            .set_radio_var("gps", if on { "1" } else { "0" })?)
    }

    /// Seconds between GPS fixes (0..=86,400).
    pub fn set_gps_interval(&self, seconds: u32) -> Result<(), LichenError> {
        if seconds > 86_400 {
            return Err(LichenError::InvalidSetting);
        }
        Ok(self
            .lock()
            .core
            .set_radio_var("gps_interval", &seconds.to_string())?)
    }

    /// Which advert types become contacts by themselves (with manual add), and from how far.
    pub fn set_autoadd(&self, mask: u8, max_hops: u8) -> Result<(), LichenError> {
        Ok(self.lock().core.set_autoadd(mask, max_hops)?)
    }

    /// The Bluetooth PIN, from the next boot: 0 for the default, else six digits.
    pub fn set_ble_pin(&self, pin: u32) -> Result<(), LichenError> {
        Ok(self.lock().core.set_device_pin(pin)?)
    }

    /// Restarts the radio; the connection drops and comes back.
    pub fn reboot_radio(&self) {
        self.lock().core.reboot_radio();
    }
}
