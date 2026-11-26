//! What the core reads from the radio after `APP_START` (protocol §1.1): the steps the apps
//! show while a connection fills its lists ("Reading the radio · contacts 34 of 120"). Read
//! from the radio's own answers only: no command of its own, no guess of the time left.

use crate::core::{Core, Event};
use lc_proto::Millis;

/// A step of the reading, in the order its commands go out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioReading {
    /// The radio's identity and settings (`DEVICE_QUERY`, the flood scope, its settings).
    Settings,
    /// Its contacts: `done` of the `total` it announced (`CONTACTS_START`).
    Contacts { done: u32, total: u32 },
    /// Its channel slots (read one by one until an empty one: no total).
    Channels,
    /// The messages that waited on it, `fetched` so far (read in turn with the channels: this
    /// step while some are coming).
    Messages { fetched: u32 },
}

/// Where the reading stands.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Reading {
    pub(crate) contacts_done: bool,
    /// The last contacts counted, shown while the list is asked for again (§16.1).
    pub(crate) last_contacts: Option<(u32, u32)>,
    /// A channel read went unanswered: the scan is over as far as the reading goes.
    pub(crate) channels_given_up: bool,
    pub(crate) fetched: u32,
    /// The queue of waiting messages is drained (or its read went unanswered).
    pub(crate) drained: bool,
    /// The step last told to the apps ([`Event::RadioReading`]), and when.
    shown: Option<RadioReading>,
    shown_at: Millis,
}

/// While counting, the apps hear of it at most this often...
const SHOW_EVERY_MS: Millis = 250;
/// ...or every this many items.
const SHOW_EVERY_ITEMS: u32 = 10;

impl Core {
    /// What is being read from the radio after `APP_START` (§1.1); `None` once it is over, and
    /// while no radio is connected. [`Event::RadioReading`] follows each step, and the count
    /// at most every 250 ms or 10 items.
    pub fn radio_reading(&self) -> Option<RadioReading> {
        let r = self.reading.as_ref()?;
        if !r.contacts_done {
            return Some(match self.contacts_count() {
                (Some(total), done) => RadioReading::Contacts {
                    done: done.min(total),
                    total,
                },
                // Asked again after a list cut short (§16.1): the last count meanwhile.
                (None, _) => r
                    .last_contacts
                    .map_or(RadioReading::Settings, |(done, total)| {
                        RadioReading::Contacts { done, total }
                    }),
            });
        }
        // The channel slots and the waiting messages are read in turn: the messages are the
        // step while some are coming, the channels while their slots are read.
        let channels_done = self.channels_known() || r.channels_given_up;
        if !r.drained && (r.fetched > 0 || channels_done) {
            return Some(RadioReading::Messages { fetched: r.fetched });
        }
        if !channels_done {
            return Some(RadioReading::Channels);
        }
        None
    }

    /// A connection starts: the reading too, from the settings.
    pub(crate) fn reading_started(&mut self, now: Millis) {
        self.reading = Some(Reading {
            shown: Some(RadioReading::Settings),
            shown_at: now,
            ..Reading::default()
        });
    }

    /// Something the reading follows moved: tell the apps when the step changed, or the count
    /// moved enough; forget the reading once it is over.
    pub(crate) fn reading_moved(&mut self, now: Millis) {
        let step = self.radio_reading();
        let Some(r) = self.reading.as_mut() else {
            return;
        };
        let Some(step) = step else {
            self.reading = None;
            self.events.push_back(Event::RadioReading);
            return;
        };
        if let RadioReading::Contacts { done, total } = step {
            r.last_contacts = Some((done, total));
        }
        let count = |s: Option<RadioReading>| match s {
            Some(RadioReading::Contacts { done, .. }) => done,
            Some(RadioReading::Messages { fetched }) => fetched,
            _ => 0,
        };
        let same_kind = r
            .shown
            .is_some_and(|s| core::mem::discriminant(&s) == core::mem::discriminant(&step));
        let tell = !same_kind
            || (r.shown != Some(step)
                && (count(Some(step)) >= count(r.shown).saturating_add(SHOW_EVERY_ITEMS)
                    || now >= r.shown_at.saturating_add(SHOW_EVERY_MS)));
        if tell {
            r.shown = Some(step);
            r.shown_at = now;
            self.events.push_back(Event::RadioReading);
        }
    }

    /// The contacts are read (whole, or cut short with no more asking).
    pub(crate) fn reading_contacts_done(&mut self, now: Millis) {
        if let Some(r) = self.reading.as_mut() {
            r.contacts_done = true;
        }
        self.reading_moved(now);
    }

    /// A waiting message came from the radio's queue.
    pub(crate) fn reading_fetched(&mut self, now: Millis) {
        if let Some(r) = self.reading.as_mut() {
            r.fetched = r.fetched.saturating_add(1);
        }
        self.reading_moved(now);
    }

    /// The queue is drained, or its read went unanswered.
    pub(crate) fn reading_drained(&mut self, now: Millis) {
        if let Some(r) = self.reading.as_mut() {
            r.drained = true;
        }
        self.reading_moved(now);
    }

    /// A channel read went unanswered.
    pub(crate) fn reading_channels_given_up(&mut self, now: Millis) {
        if let Some(r) = self.reading.as_mut() {
            r.channels_given_up = true;
        }
        self.reading_moved(now);
    }
}
