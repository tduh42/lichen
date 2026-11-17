//! Room servers and repeaters (protocol §15.8, §16.5; ADR-0043, ADR-0044): logins, a room's
//! posts, a repeater's status and neighbours, and its command line.
//!
//! The companion handles one request at a time (a login, a status or a binary request: a
//! new one replaces the one it waits for), so lichen sends them one after another. Nothing
//! here is stored except, by the app, a room's password.

use std::collections::{HashMap, VecDeque};

use lc_meshcore::Command;
use lc_meshcore::codes::txt;
use lc_meshcore::command::{PASSWORD_MAX, neighbours_request};
use lc_meshcore::server::{Neighbours, ServerStatus};
use rand_core::Rng;

use crate::core::{Core, Error, Event, Expect, PubKey, contact_kind};
use crate::plain::{PlainChat, PlainMessage};
use lc_proto::Millis;

/// No answer within the companion's estimate plus this: given up.
const ANSWER_MARGIN_MS: Millis = 4_000;
/// Without a `SENT`, a request is given up after this.
const SENT_WAIT_MS: Millis = 10_000;
/// Key bytes asked for each neighbour (§16.5).
pub const NEIGHBOUR_PREFIX: u8 = 6;
/// Pages of neighbours asked for at most (11 per answer).
const NEIGHBOUR_PAGES: u16 = 4;
/// Console lines kept per server.
const CONSOLE_MAX: usize = 200;
/// Longest console command (the server keeps a 160-byte text).
pub const COMMAND_MAX: usize = lc_meshcore::command::TEXT_MAX;

/// What a login gave us (role bits of the server's permissions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A room's reader who may not post, or a repeater's guest.
    Guest,
    ReadOnly,
    ReadWrite,
    Admin,
}

impl Role {
    const fn of(permissions: Option<u8>, admin: u8) -> Self {
        match permissions {
            Some(p) => match p & 3 {
                3 => Self::Admin,
                2 => Self::ReadWrite,
                1 => Self::ReadOnly,
                _ => Self::Guest,
            },
            None if admin == 1 => Self::Admin,
            None => Self::Guest,
        }
    }
}

/// Where a login stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Login {
    /// Not asked in this connection.
    Out,
    /// Sent (or waiting for the radio).
    Pending,
    In(Role),
    /// The server refused it.
    Refused,
    /// No answer: a wrong password (servers do not answer one), or out of reach.
    NoAnswer,
}

/// One of a repeater's neighbours, named when the radio knows its key prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Neighbour {
    pub prefix: Vec<u8>,
    pub name: Option<String>,
    pub heard_ago_s: u32,
    pub snr4: i8,
}

/// A line of a server's command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleLine {
    /// Ours (a command), else the server's reply.
    pub outgoing: bool,
    pub text: String,
    /// Unix seconds (the sender's clock), 0 unknown.
    pub at: u32,
}

/// What lichen knows of a room server or repeater in this connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub login: Login,
    pub status: Option<ServerStatus>,
    /// A status request is out.
    pub status_pending: bool,
    pub neighbours: Option<Vec<Neighbour>>,
    /// Neighbours it knows in all (it may know more than it sent).
    pub neighbours_total: u16,
    pub neighbours_pending: bool,
    /// The last request got no answer.
    pub no_answer: bool,
    pub console: Vec<ConsoleLine>,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            login: Login::Out,
            status: None,
            status_pending: false,
            neighbours: None,
            neighbours_total: 0,
            neighbours_pending: false,
            no_answer: false,
            console: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Login(Vec<u8>),
    Status,
    Neighbours { offset: u16 },
}

#[derive(Clone, Debug)]
struct InFlight {
    key: PubKey,
    request: Request,
    /// `SENT`'s tag (a binary request's answer carries it).
    tag: Option<u32>,
    deadline: Millis,
}

/// The servers' state.
#[derive(Debug, Default)]
pub(crate) struct Servers {
    servers: HashMap<PubKey, Server>,
    queue: VecDeque<(PubKey, Request)>,
    in_flight: Option<InFlight>,
    /// Rooms the user logs in to at each connection, with their password (kept by the app).
    rooms: Vec<(PubKey, String)>,
    /// Their logins went out in this connection.
    rooms_asked: bool,
}

fn text_of(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl Core {
    // ----- app requests ----------------------------------------------------------------

    /// What lichen knows of room server or repeater `node` in this connection.
    pub fn server(&self, node: &PubKey) -> Option<&Server> {
        self.servers.servers.get(node)
    }

    /// Logs in to room server or repeater `node` (§15.8, §16.5); an empty password for a
    /// server that already lists us. A room then sends the posts we have not seen.
    pub fn login(&mut self, node: &PubKey, password: &str, now: Millis) -> Result<(), Error> {
        if password.len() > PASSWORD_MAX || password.contains('\0') {
            return Err(Error::TooLarge);
        }
        self.server_request(node, Request::Login(password.as_bytes().to_vec()), now)?;
        self.servers.servers.entry(*node).or_default().login = Login::Pending;
        Ok(())
    }

    /// The rooms to log in to at each connection, once the radio's contacts are read, so
    /// that their posts since the last time come (§15.8). The app keeps them.
    pub fn set_room_logins(&mut self, rooms: Vec<(PubKey, String)>) {
        self.servers.rooms = rooms;
    }

    /// The radio's contacts were read: log in to the rooms the user keeps, once per
    /// connection.
    pub(crate) fn servers_contacts_read(&mut self, now: Millis) {
        if self.servers.rooms_asked {
            return;
        }
        self.servers.rooms_asked = true;
        let rooms: Vec<(PubKey, String)> = self
            .servers
            .rooms
            .iter()
            .filter(|(k, _)| {
                self.contacts
                    .get(k)
                    .is_some_and(|c| c.kind == contact_kind::ROOM)
            })
            .cloned()
            .collect();
        for (k, pw) in rooms {
            let _ = self.login(&k, &pw, now);
        }
    }

    /// Forgets our login to `node`: the radio stops keeping it alive.
    pub fn logout(&mut self, node: &PubKey) {
        self.servers.queue.retain(|(k, _)| k != node);
        if let Some(s) = self.servers.servers.get_mut(node) {
            s.login = Login::Out;
        }
        if self.online {
            self.command(&Command::Logout { key: node }, Expect::Ok);
        }
        self.events.push_back(Event::NodeTool { node: *node });
    }

    /// Asks `node` for its status (§16.5).
    pub fn request_status(&mut self, node: &PubKey, now: Millis) -> Result<(), Error> {
        self.server_request(node, Request::Status, now)?;
        self.servers
            .servers
            .entry(*node)
            .or_default()
            .status_pending = true;
        Ok(())
    }

    /// Asks repeater `node` for the neighbours it hears, the strongest first (§16.5).
    pub fn request_neighbours(&mut self, node: &PubKey, now: Millis) -> Result<(), Error> {
        self.server_request(node, Request::Neighbours { offset: 0 }, now)?;
        let s = self.servers.servers.entry(*node).or_default();
        s.neighbours_pending = true;
        s.neighbours = Some(Vec::new());
        Ok(())
    }

    /// Sends a command to `node`'s command line (§16.5): it answers admins only.
    pub fn send_command(
        &mut self,
        node: &PubKey,
        command: &str,
        sent_at: u32,
        now: Millis,
    ) -> Result<(), Error> {
        let command = command.trim();
        if command.is_empty() {
            return Err(Error::Empty);
        }
        if command.len() > COMMAND_MAX {
            return Err(Error::TooLarge);
        }
        let path = self.server_path(node)?;
        let Some(prefix) = node.get(..6).and_then(|p| <[u8; 6]>::try_from(p).ok()) else {
            return Err(Error::UnknownContact);
        };
        let airtime = self.airtime_ms(crate::plain::direct_bytes(command.len(), path.0));
        if !self.budget_allows(now, airtime) {
            return Err(Error::OverBudget {
                wait_ms: self.budget_wait(now, airtime),
            });
        }
        self.command(
            &Command::SendTxtMsg {
                kind: txt::CLI_DATA,
                attempt: 0,
                ts: sent_at,
                prefix: &prefix,
                text: command.as_bytes(),
            },
            Expect::Cli(*node),
        );
        self.record_airtime(now, airtime, path.1);
        self.console_line(node, true, command.to_owned(), sent_at);
        self.tick(now);
        Ok(())
    }

    // ----- plumbing --------------------------------------------------------------------

    /// The bytes and hops of the radio's path to server `node`; refused when lichen does not
    /// know it or the radio is away.
    fn server_path(&self, node: &PubKey) -> Result<(usize, Option<u8>), Error> {
        let (kind, path) = self
            .contacts
            .get(node)
            .map(|c| (c.kind, c.out_path.clone()))
            .ok_or(Error::UnknownContact)?;
        if kind == contact_kind::CHAT {
            return Err(Error::NotAllowed);
        }
        if !self.online {
            return Err(Error::NoIdentity);
        }
        Ok((
            path.as_ref().map_or(0, |p| p.bytes.len()),
            path.as_ref().map(crate::OwnedPath::hops),
        ))
    }

    fn server_request(&mut self, node: &PubKey, r: Request, now: Millis) -> Result<(), Error> {
        self.server_path(node)?;
        let busy = |(k, q): &(PubKey, Request)| {
            k == node && std::mem::discriminant(q) == std::mem::discriminant(&r)
        };
        let in_flight = self.servers.in_flight.as_ref().is_some_and(|f| {
            f.key == *node && std::mem::discriminant(&f.request) == std::mem::discriminant(&r)
        });
        if in_flight || self.servers.queue.iter().any(busy) {
            return Err(Error::TooSoon { wait_ms: 0 });
        }
        self.servers.queue.push_back((*node, r));
        self.servers.servers.entry(*node).or_default().no_answer = false;
        self.events.push_back(Event::NodeTool { node: *node });
        self.tick(now);
        Ok(())
    }

    /// Sends the next request when none is out.
    pub(crate) fn tick_servers(&mut self, now: Millis) {
        if let Some(f) = &self.servers.in_flight {
            if now < f.deadline {
                return;
            }
            if let Some(f) = self.servers.in_flight.take() {
                self.server_failed(&f.key, &f.request, Login::NoAnswer);
            }
        }
        let Some((key, r)) = self.servers.queue.front().cloned() else {
            return;
        };
        let Ok((path_bytes, hops)) = self.server_path(&key) else {
            self.servers.queue.pop_front();
            self.server_failed(&key, &r, Login::NoAnswer);
            return;
        };
        // Our request on the air: an encrypted request of a few blocks (§16.5).
        let len = match &r {
            Request::Login(pw) => 2 + path_bytes + 32 + 1 + 2 + (8 + pw.len()).div_ceil(16) * 16,
            Request::Status => 2 + path_bytes + 2 + 2 + 16,
            Request::Neighbours { .. } => 2 + path_bytes + 2 + 2 + 16,
        };
        let airtime = self.airtime_ms(len);
        if !self.budget_allows(now, airtime) {
            return;
        }
        self.servers.queue.pop_front();
        match &r {
            Request::Login(pw) => self.command(
                &Command::SendLogin {
                    key: &key,
                    password: pw,
                },
                Expect::Server,
            ),
            Request::Status => self.command(&Command::SendStatusReq { key: &key }, Expect::Server),
            Request::Neighbours { offset } => {
                let mut random = [0u8; 4];
                self.rng.fill_bytes(&mut random);
                let data = neighbours_request(255, *offset, 2, NEIGHBOUR_PREFIX, random);
                self.command(
                    &Command::SendBinaryReq {
                        key: &key,
                        data: &data,
                    },
                    Expect::Server,
                );
            }
        }
        self.record_airtime(now, airtime, hops);
        self.servers.in_flight = Some(InFlight {
            key,
            request: r,
            tag: None,
            deadline: now + SENT_WAIT_MS,
        });
    }

    pub(crate) fn next_server_wakeup(&self) -> Option<Millis> {
        match &self.servers.in_flight {
            Some(f) => Some(f.deadline),
            None => (!self.servers.queue.is_empty()).then_some(0),
        }
    }

    fn server_failed(&mut self, key: &PubKey, r: &Request, login: Login) {
        let s = self.servers.servers.entry(*key).or_default();
        match r {
            Request::Login(_) => s.login = login,
            Request::Status => s.status_pending = false,
            Request::Neighbours { .. } => s.neighbours_pending = false,
        }
        s.no_answer = login == Login::NoAnswer;
        self.events.push_back(Event::NodeTool { node: *key });
    }

    /// `SENT` for the request out: its tag and the companion's estimate.
    pub(crate) fn on_server_sent(&mut self, tag: u32, timeout_ms: u32, now: Millis) {
        if let Some(f) = self.servers.in_flight.as_mut() {
            f.tag = Some(tag);
            f.deadline = now + Millis::from(timeout_ms) + ANSWER_MARGIN_MS;
        }
    }

    /// The companion refused the request (`ERR`: the server is not in its contacts).
    pub(crate) fn on_server_refused(&mut self) {
        if let Some(f) = self.servers.in_flight.take() {
            self.server_failed(&f.key, &f.request, Login::Refused);
        }
    }

    /// The in-flight request to the server whose key starts with `prefix`, if it is `pick`.
    fn answered(&mut self, prefix: &[u8], pick: fn(&Request) -> bool) -> Option<InFlight> {
        let hit = self
            .servers
            .in_flight
            .as_ref()
            .is_some_and(|f| f.key.starts_with(prefix) && pick(&f.request));
        if hit {
            self.servers.in_flight.take()
        } else {
            None
        }
    }

    /// Push `LOGIN_SUCCESS` or `LOGIN_FAIL` (`role` `None`).
    pub(crate) fn on_login(&mut self, prefix: &[u8; 6], role: Option<(Option<u8>, u8)>) {
        let Some(f) = self.answered(prefix, |r| matches!(r, Request::Login(_))) else {
            return;
        };
        let s = self.servers.servers.entry(f.key).or_default();
        s.login = match role {
            Some((permissions, admin)) => Login::In(Role::of(permissions, admin)),
            None => Login::Refused,
        };
        s.no_answer = false;
        self.events.push_back(Event::NodeTool { node: f.key });
    }

    /// Push `STATUS_RESPONSE`.
    pub(crate) fn on_status(&mut self, prefix: &[u8; 6], data: &[u8]) {
        let Some(f) = self.answered(prefix, |r| *r == Request::Status) else {
            return;
        };
        let s = self.servers.servers.entry(f.key).or_default();
        s.status_pending = false;
        s.no_answer = false;
        if let Some(st) = ServerStatus::decode(data) {
            s.status = Some(st);
        }
        self.events.push_back(Event::NodeTool { node: f.key });
    }

    /// Push `BINARY_RESPONSE`: a page of neighbours.
    pub(crate) fn on_binary(&mut self, tag: u32, data: &[u8], now: Millis) {
        let Some(f) = self.servers.in_flight.as_ref() else {
            return;
        };
        let Request::Neighbours { offset } = f.request else {
            return;
        };
        if f.tag != Some(tag) {
            return;
        }
        let key = f.key;
        self.servers.in_flight = None;
        let Some(page) = Neighbours::decode(data, usize::from(NEIGHBOUR_PREFIX)) else {
            self.server_failed(&key, &Request::Neighbours { offset }, Login::Refused);
            return;
        };
        let got: Vec<Neighbour> = page
            .iter()
            .map(|n| Neighbour {
                prefix: n.prefix.to_vec(),
                name: self
                    .nodes()
                    .into_iter()
                    .find(|x| x.key.starts_with(n.prefix))
                    .map(|x| x.name),
                heard_ago_s: n.heard_ago_s,
                snr4: n.snr4,
            })
            .collect();
        let next = offset.saturating_add(page.count);
        let more =
            page.count > 0 && next < page.total && next / u16::max(page.count, 1) < NEIGHBOUR_PAGES;
        let s = self.servers.servers.entry(key).or_default();
        s.neighbours.get_or_insert_with(Vec::new).extend(got);
        s.neighbours_total = page.total;
        s.neighbours_pending = more;
        s.no_answer = false;
        if more {
            self.servers
                .queue
                .push_front((key, Request::Neighbours { offset: next }));
        }
        self.events.push_back(Event::NodeTool { node: key });
        self.tick_servers(now);
    }

    /// The companion refused a console command.
    pub(crate) fn on_cli_refused(&mut self, node: &PubKey) {
        self.console_line(node, false, String::new(), 0);
    }

    fn console_line(&mut self, node: &PubKey, outgoing: bool, text: String, at: u32) {
        let s = self.servers.servers.entry(*node).or_default();
        s.console.push(ConsoleLine { outgoing, text, at });
        if s.console.len() > CONSOLE_MAX {
            s.console.remove(0);
        }
        self.events.push_back(Event::NodeTool { node: *node });
    }

    /// A `CONTACT_MSG` of type 1: a server's command-line reply (§16.5).
    pub(crate) fn on_cli_reply(&mut self, prefix: &[u8; 6], ts: u32, text: &[u8]) {
        // Servers answer commands; a person's radio does not (dropped).
        let Some(key) = self
            .contacts
            .values()
            .find(|c| c.key.starts_with(prefix) && !c.is_person())
            .map(|c| c.key)
        else {
            return;
        };
        self.console_line(&key, false, text_of(text), ts);
    }

    /// A `CONTACT_MSG` of type 2: a room server's post, its author's key prefix first
    /// (§15.8).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn on_room_post(
        &mut self,
        prefix: &[u8; 6],
        ts: u32,
        text: &[u8],
        snr4: Option<i8>,
        path_len: u8,
        now: Millis,
    ) {
        // Only a room server signs posts: one from another node is dropped.
        let Some((room, room_name)) = self
            .contacts
            .values()
            .find(|c| c.key.starts_with(prefix) && c.kind == contact_kind::ROOM)
            .map(|c| (c.key, c.name.clone()))
        else {
            return;
        };
        let (Some(author), Some(body)) = (text.get(..4), text.get(4..)) else {
            return;
        };
        let body = text_of(body);
        let chat = PlainChat::Room(room);
        if self.plain_seen(&chat, ts, &body, now) {
            return;
        }
        // The room's own posts (its system messages) carry its key.
        let from = self
            .contacts
            .values()
            .find(|c| c.key.starts_with(author))
            .map(|c| (c.key, c.name.clone()));
        let label = match &from {
            Some((k, _)) if *k == room => room_name,
            Some((_, name)) => name.clone(),
            None => author.iter().map(|b| format!("{b:02x}")).collect(),
        };
        self.events.push_back(Event::PlainReceived(PlainMessage {
            chat,
            from: from.map(|(k, _)| k),
            label: Some(label),
            text: body,
            sent_at: ts,
            snr4,
            hops: (path_len != 0xFF).then_some(path_len & 0x3F),
        }));
    }

    /// The radio is gone: what waited for an answer has none (pushes are lost); logins are
    /// asked again at the next connection.
    pub(crate) fn servers_connected(&mut self) {
        self.servers.rooms_asked = false;
    }

    pub(crate) fn servers_disconnected(&mut self) {
        if let Some(f) = self.servers.in_flight.take() {
            self.server_failed(&f.key, &f.request, Login::NoAnswer);
        }
        for (k, r) in std::mem::take(&mut self.servers.queue) {
            self.server_failed(&k, &r, Login::NoAnswer);
        }
        for s in self.servers.servers.values_mut() {
            if matches!(s.login, Login::In(_) | Login::Pending) {
                s.login = Login::Out;
            }
        }
    }
}
