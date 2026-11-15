//! Search mode (protocol.md §11).

use super::*;

#[uniffi::export]
impl LichenCore {
    // ----- search (protocol.md §11) -----

    /// Starts searching for member `target` (node or identity) of group `gid`.
    pub fn start_search(
        &self,
        gid: Vec<u8>,
        target: Vec<u8>,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let (g, k) = (self::gid(&gid)?, key(&target)?);
        let mut i = self.lock();
        i.core.start_search(&g, &k, now_ms)?;
        i.persist();
        Ok(())
    }

    pub fn stop_search(&self, target: Vec<u8>) -> Result<(), LichenError> {
        let k = key(&target)?;
        self.lock().core.stop_search(&k);
        Ok(())
    }

    pub fn searches(&self, now_ms: u64) -> Vec<SearchState> {
        self.lock()
            .core
            .searches(now_ms)
            .into_iter()
            .map(|s| SearchState {
                gid: s.gid.to_vec(),
                node: s.node.to_vec(),
                identity: s.target.to_vec(),
                measurements: u32::try_from(s.measurements).unwrap_or(u32::MAX),
                tx_dbm: s.tx_dbm,
                battery: s.battery,
                last_heard_s: s.last_heard,
                last_fix: s.last_fix.map(|f| SearchFix {
                    at_s: f.at,
                    lat: f.lat,
                    lon: f.lon,
                    radius_m: f.radius_m,
                }),
                ping_in_ms: s.ping_in_ms,
            })
            .collect()
    }

    /// Asks the target to transmit now (`TooSoon` with the wait otherwise).
    pub fn search_ping(&self, target: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&target)?;
        let mut i = self.lock();
        i.core.search_ping(&k, now_ms)?;
        i.persist();
        Ok(())
    }

    /// A bearing to the target from here, degrees from true north; `yagi`: taken with a
    /// directional antenna (else by turning around, the body shadowing the signal).
    pub fn search_bearing(
        &self,
        target: Vec<u8>,
        deg: f64,
        yagi: bool,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&target)?;
        let class = if yagi {
            BearingClass::Yagi
        } else {
            BearingClass::Body
        };
        let mut i = self.lock();
        i.core.search_bearing(&k, deg, class, now_ms)?;
        i.persist();
        Ok(())
    }

    /// Asks the target to set its TX power to `dbm` for `minutes` (close range).
    pub fn search_lower_power(
        &self,
        target: Vec<u8>,
        dbm: i8,
        minutes: u8,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&target)?;
        let mut i = self.lock();
        i.core.search_lower_power(&k, dbm, minutes, now_ms)?;
        i.persist();
        Ok(())
    }

    pub fn search_estimate(&self, target: Vec<u8>) -> Option<SearchEstimate> {
        let k = key(&target).ok()?;
        let e = self.lock().core.search_estimate(&k)?;
        Some(SearchEstimate {
            lat: e.lat,
            lon: e.lon,
            major68_m: e.major68_m,
            minor68_m: e.minor68_m,
            major95_m: e.major95_m,
            minor95_m: e.minor95_m,
            azimuth_deg: e.azimuth_deg,
            heat_north: e.heat.north,
            heat_south: e.heat.south,
            heat_west: e.heat.west,
            heat_east: e.heat.east,
            heat_cols: e.heat.cols,
            heat_rows: e.heat.rows,
            heat: e.heat.cells,
            observations: u32::try_from(e.observations).unwrap_or(u32::MAX),
            calibrated: e.model != lc_core::PathLoss::DEFAULT,
        })
    }

    /// Every reception of the target, newest first.
    pub fn search_measurements(&self, target: Vec<u8>) -> Vec<SearchMeasurement> {
        let Ok(k) = key(&target) else {
            return Vec::new();
        };
        let i = self.lock();
        let me = i.core.key().copied();
        let mut v: Vec<SearchMeasurement> = i
            .core
            .search_measurements(&k)
            .iter()
            .map(|m| SearchMeasurement {
                by: i.node_of(&m.by).unwrap_or(m.by).to_vec(),
                mine: Some(m.by) == me,
                at_s: m.at,
                lat: m.lat,
                lon: m.lon,
                accuracy_m: m.accuracy_m,
                rssi: m.rssi,
                snr_db: snr(m.snr4),
                noise: m.noise,
                estimated: m.estimated,
                saturated: m.saturated,
                bearing_deg: m.bearing.map(|b| b.deg),
                bearing_yagi: m.bearing.is_some_and(|b| b.class == BearingClass::Yagi),
            })
            .collect();
        v.reverse();
        v
    }

    /// The target's receptions as CSV (field tests, ADR-0016), oldest first.
    pub fn search_csv(&self, target: Vec<u8>) -> String {
        let Ok(k) = key(&target) else {
            return String::new();
        };
        let i = self.lock();
        let me = i.core.key().copied();
        let mut out = String::from(
            "unix_s,searcher,mine,lat,lon,accuracy_m,rssi_dbm,snr_db,noise_dbm,estimated,saturated,\
             bearing_deg,bearing_class,target_tx_dbm\n",
        );
        let opt = |v: Option<i8>| v.map_or(String::new(), |x| x.to_string());
        for m in i.core.search_measurements(&k) {
            let by = i.node_of(&m.by).unwrap_or(m.by);
            let (deg, class) = m.bearing.map_or((String::new(), ""), |b| {
                let c = match b.class {
                    BearingClass::Yagi => "yagi",
                    BearingClass::Body => "body",
                    BearingClass::Other => "other",
                };
                (b.deg.to_string(), c)
            });
            out.push_str(&format!(
                "{},{},{},{:.6},{:.6},{:.0},{},{:.2},{},{},{},{},{},{}\n",
                m.at,
                by.iter()
                    .take(4)
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                u8::from(Some(m.by) == me),
                m.lat,
                m.lon,
                m.accuracy_m,
                opt(m.rssi),
                f32::from(m.snr4) / 4.0,
                opt(m.noise),
                u8::from(m.estimated),
                u8::from(m.saturated),
                deg,
                class,
                opt(m.tx_dbm),
            ));
        }
        out
    }

    /// Members searching for us (the last ten minutes).
    pub fn searched_by(&self, now_ms: u64) -> Vec<SearchedBy> {
        self.lock()
            .core
            .searched_by(now_ms)
            .into_iter()
            .map(|(g, by)| SearchedBy {
                gid: g.to_vec(),
                by: by.to_vec(),
            })
            .collect()
    }

    /// The phone's battery level (0..=100), sent in our BEACONs.
    pub fn set_battery(&self, percent: Option<u8>) {
        self.lock().core.set_battery(percent);
    }
}
