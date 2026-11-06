//! Duty-cycle budget (ADR-0007): the regulatory rule for the device's frequency, from
//! `config/eu868.toml`, and a one-hour sliding count of our own transmissions.

use std::collections::VecDeque;

use lc_proto::Millis;
use serde::Deserialize;

/// The EU 863-870 MHz profile shipped with lichen.
pub const EU868: &str = include_str!("../../../config/eu868.toml");

#[derive(Debug, Deserialize)]
struct Profile {
    window_s: u64,
    rule: Vec<Rule>,
}

/// One sub-band rule.
#[derive(Clone, Debug, Deserialize)]
pub struct Rule {
    pub name: String,
    pub low_hz: u64,
    pub high_hz: u64,
    pub max_erp_mw: f64,
    pub duty_percent: f64,
}

/// Sliding-window airtime budget.
#[derive(Clone, Debug)]
pub struct Budget {
    rule: Option<Rule>,
    window_ms: Millis,
    limit_ms: Millis,
    log: VecDeque<(Millis, Millis)>,
}

impl Budget {
    /// The budget for `freq_hz` under `profile` (TOML). The most permissive matching rule wins,
    /// as the profile says. Outside every rule, the budget falls back to 0.1 %.
    pub fn for_frequency(profile: &str, freq_hz: u64) -> Result<Self, toml::de::Error> {
        let p: Profile = toml::from_str(profile)?;
        let rule = p
            .rule
            .into_iter()
            .filter(|r| r.low_hz <= freq_hz && freq_hz <= r.high_hz)
            .max_by(|a, b| a.duty_percent.total_cmp(&b.duty_percent));
        let window_ms = p.window_s * 1000;
        let duty = rule.as_ref().map_or(0.1, |r| r.duty_percent);
        let limit_ms = (window_ms as f64 * duty / 100.0) as Millis;
        Ok(Self {
            rule,
            window_ms,
            limit_ms,
            log: VecDeque::new(),
        })
    }

    pub fn rule(&self) -> Option<&Rule> {
        self.rule.as_ref()
    }

    pub const fn limit_ms(&self) -> Millis {
        self.limit_ms
    }

    fn purge(&mut self, now: Millis) {
        while self
            .log
            .front()
            .is_some_and(|&(t, _)| t + self.window_ms <= now)
        {
            self.log.pop_front();
        }
    }

    /// Airtime spent in the last window.
    pub fn used_ms(&mut self, now: Millis) -> Millis {
        self.purge(now);
        self.log.iter().map(|&(_, a)| a).sum()
    }

    pub fn can_transmit(&mut self, now: Millis, airtime_ms: Millis) -> bool {
        self.used_ms(now) + airtime_ms <= self.limit_ms
    }

    pub fn record(&mut self, now: Millis, airtime_ms: Millis) {
        self.log.push_back((now, airtime_ms));
    }

    /// Earliest time a transmission of `airtime_ms` fits.
    pub fn next_opportunity(&mut self, now: Millis, airtime_ms: Millis) -> Millis {
        let mut used = self.used_ms(now);
        for &(t, a) in &self.log {
            if used + airtime_ms <= self.limit_ms {
                break;
            }
            used -= a;
            if used + airtime_ms <= self.limit_ms {
                return t + self.window_ms;
            }
        }
        now
    }
}
