//! Room servers and repeaters (protocol §15.8, §16.5; ADR-0043, ADR-0044): logins, status,
//! neighbours, the command line. A room's posts are a conversation (`ChatRef::Room`).

use super::*;

/// Store setting: the rooms logged in to at each connection, `key (32) ‖ len ‖ password`
/// each.
pub(crate) const ROOM_LOGINS: &str = "room_logins";

pub(crate) fn decode_room_logins(v: &[u8]) -> Vec<(PubKey, String)> {
    let mut out = Vec::new();
    let mut rest = v;
    while let (Some(k), Some(&len)) = (rest.get(..32), rest.get(32)) {
        let end = 33 + usize::from(len);
        let (Ok(key), Some(pw)) = (<PubKey>::try_from(k), rest.get(33..end)) else {
            break;
        };
        out.push((key, String::from_utf8_lossy(pw).into_owned()));
        rest = rest.get(end..).unwrap_or(&[]);
    }
    out
}

fn encode_room_logins(rooms: &[(PubKey, String)]) -> Vec<u8> {
    let mut v = Vec::new();
    for (k, pw) in rooms {
        v.extend_from_slice(k);
        v.push(u8::try_from(pw.len()).unwrap_or(0));
        v.extend_from_slice(pw.as_bytes());
    }
    v
}

impl Inner {
    pub(crate) fn room_logins(&self) -> Vec<(PubKey, String)> {
        self.store
            .as_ref()
            .and_then(|s| s.setting(ROOM_LOGINS).ok().flatten())
            .map(|v| decode_room_logins(&v))
            .unwrap_or_default()
    }

    fn set_room_login(&mut self, node: &PubKey, password: Option<&str>) {
        let mut rooms = self.room_logins();
        rooms.retain(|(k, _)| k != node);
        if let Some(pw) = password {
            rooms.push((*node, pw.to_owned()));
        }
        if let Some(s) = &self.store {
            let _ = s.set_setting(ROOM_LOGINS, Some(&encode_room_logins(&rooms)));
        }
        self.core.set_room_logins(rooms);
    }
}

/// Where a login to a server stands, and the role it gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ServerLogin {
    /// Not logged in during this connection.
    Out,
    Pending,
    /// Logged in: a room's reader who may not post, or a repeater's guest.
    Guest,
    ReadOnly,
    /// May post in a room.
    ReadWrite,
    Admin,
    /// The server refused it.
    Refused,
    /// No answer: a wrong password (servers do not answer one), or out of reach.
    NoAnswer,
}

/// A server's statistics (§16.5).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ServerStats {
    pub battery_mv: u16,
    pub uptime_s: u32,
    pub tx_queue: u16,
    pub noise_floor: i16,
    pub last_rssi: i16,
    pub last_snr_db: f32,
    pub packets_recv: u32,
    pub packets_sent: u32,
    /// Seconds spent transmitting, and receiving (repeaters).
    pub air_time_s: u32,
    pub rx_air_time_s: Option<u32>,
    pub sent_flood: u32,
    pub sent_direct: u32,
    pub recv_flood: u32,
    pub recv_direct: u32,
    pub flood_dups: u16,
    pub direct_dups: u16,
    pub recv_errors: Option<u32>,
    /// A room's posts kept and sent to its clients.
    pub posted: Option<u16>,
    pub post_pushes: Option<u16>,
}

/// One of a repeater's neighbours.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NeighbourInfo {
    /// The first bytes of its key (hex).
    pub prefix: String,
    /// Its name, when the radio knows it.
    pub name: Option<String>,
    pub heard_ago_s: u32,
    pub snr_db: f32,
}

/// A line of a server's command line.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ConsoleEntry {
    /// Ours (a command), else the server's reply; an empty reply of ours: refused by the
    /// radio.
    pub outgoing: bool,
    pub text: String,
    /// Unix seconds by the sender's clock, 0 unknown.
    pub at_s: u64,
}

/// What lichen knows of a room server or repeater in this connection.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ServerInfo {
    pub login: ServerLogin,
    pub stats: Option<ServerStats>,
    pub stats_pending: bool,
    pub neighbours: Option<Vec<NeighbourInfo>>,
    /// Neighbours it knows in all (it may send fewer).
    pub neighbours_total: u16,
    pub neighbours_pending: bool,
    /// The last request got no answer.
    pub no_answer: bool,
    pub console: Vec<ConsoleEntry>,
    /// A room whose password this phone keeps: it logs in at each connection.
    pub remembered: bool,
}

impl ServerInfo {
    fn of(s: Option<&lc_core::Server>, remembered: bool) -> Self {
        let d = lc_core::Server::default();
        let s = s.unwrap_or(&d);
        Self {
            login: match s.login {
                lc_core::Login::Out => ServerLogin::Out,
                lc_core::Login::Pending => ServerLogin::Pending,
                lc_core::Login::In(lc_core::Role::Guest) => ServerLogin::Guest,
                lc_core::Login::In(lc_core::Role::ReadOnly) => ServerLogin::ReadOnly,
                lc_core::Login::In(lc_core::Role::ReadWrite) => ServerLogin::ReadWrite,
                lc_core::Login::In(lc_core::Role::Admin) => ServerLogin::Admin,
                lc_core::Login::Refused => ServerLogin::Refused,
                lc_core::Login::NoAnswer => ServerLogin::NoAnswer,
            },
            stats: s.status.map(|st| ServerStats {
                battery_mv: st.battery_mv,
                uptime_s: st.uptime_s,
                tx_queue: st.tx_queue,
                noise_floor: st.noise_floor,
                last_rssi: st.last_rssi,
                last_snr_db: f32::from(st.last_snr4) / 4.0,
                packets_recv: st.packets_recv,
                packets_sent: st.packets_sent,
                air_time_s: st.air_time_s,
                rx_air_time_s: st.rx_air_time_s,
                sent_flood: st.sent_flood,
                sent_direct: st.sent_direct,
                recv_flood: st.recv_flood,
                recv_direct: st.recv_direct,
                flood_dups: st.flood_dups,
                direct_dups: st.direct_dups,
                recv_errors: st.recv_errors,
                posted: st.posted,
                post_pushes: st.post_pushes,
            }),
            stats_pending: s.status_pending,
            neighbours: s.neighbours.as_ref().map(|v| {
                v.iter()
                    .map(|n| NeighbourInfo {
                        prefix: crate::api_mesh::hex(&n.prefix),
                        name: n.name.clone(),
                        heard_ago_s: n.heard_ago_s,
                        snr_db: snr(n.snr4),
                    })
                    .collect()
            }),
            neighbours_total: s.neighbours_total,
            neighbours_pending: s.neighbours_pending,
            no_answer: s.no_answer,
            console: s
                .console
                .iter()
                .map(|l| ConsoleEntry {
                    outgoing: l.outgoing,
                    text: l.text.clone(),
                    at_s: u64::from(l.at),
                })
                .collect(),
            remembered,
        }
    }
}

#[uniffi::export]
impl LichenCore {
    /// What lichen knows of room server or repeater `node` (`NodeUpdated` follows changes).
    pub fn server(&self, node: Vec<u8>) -> Result<ServerInfo, LichenError> {
        let k = key(&node)?;
        let i = self.lock();
        let remembered = i.room_logins().iter().any(|(r, _)| *r == k);
        Ok(ServerInfo::of(i.core.server(&k), remembered))
    }

    /// Logs in to room server or repeater `node` (at most 15 bytes of password; empty for a
    /// server that already lists us). For a room, `remember` keeps the password on this
    /// phone and logs in at each connection, so that its posts since the last time come; a
    /// repeater's password is never kept.
    pub fn server_login(
        &self,
        node: Vec<u8>,
        password: String,
        remember: bool,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        i.core.login(&k, &password, now_ms)?;
        let room = i
            .core
            .contacts()
            .any(|c| c.key == k && c.kind == lc_core::contact_kind::ROOM);
        i.set_room_login(&k, (room && remember).then_some(password.as_str()));
        Ok(())
    }

    /// Logs out of `node` and forgets its password.
    pub fn server_logout(&self, node: Vec<u8>) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.core.logout(&k);
        i.set_room_login(&k, None);
        Ok(())
    }

    /// Asks `node` for its statistics.
    pub fn request_server_stats(&self, node: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        Ok(i.core.request_status(&k, now_ms)?)
    }

    /// Asks repeater `node` for the neighbours it hears, the strongest first.
    pub fn request_neighbours(&self, node: Vec<u8>, now_ms: u64) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        Ok(i.core.request_neighbours(&k, now_ms)?)
    }

    /// Sends `command` to `node`'s command line (admins only); its reply joins the console.
    pub fn send_server_command(
        &self,
        node: Vec<u8>,
        command: String,
        sent_at: u32,
        now_ms: u64,
    ) -> Result<(), LichenError> {
        let k = key(&node)?;
        let mut i = self.lock();
        i.last_now = now_ms;
        Ok(i.core.send_command(&k, &command, sent_at, now_ms)?)
    }
}
