//! Vectors copied from oxymesh (`testdata/meshcore/`): independent Python oracle for on-air
//! frames and ECDH, meshcore.js for the companion protocol.

use lc_meshcore::air::{Packet, Route};
use lc_meshcore::ecdh::{expand_seed, public_key, shared_secret};
use lc_meshcore::{Command, Path, Response};
use toml::{Table, Value};

fn load(name: &str) -> Table {
    let path = format!(
        "{}/../../testdata/meshcore/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap().parse().unwrap()
}

fn entries<'a>(t: &'a Table, kind: &str) -> &'a Vec<Value> {
    t[kind].as_array().unwrap()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap()
}

fn h(v: &Value, k: &str) -> Vec<u8> {
    hex::decode(s(v, k)).unwrap()
}

fn arr<const N: usize>(b: &[u8]) -> [u8; N] {
    b.try_into().unwrap()
}

#[test]
fn identities_and_ecdh() {
    let t = load("air.toml");
    let mut ids = std::collections::HashMap::new();
    for v in entries(&t, "identity") {
        let prv = expand_seed(&arr(&h(v, "seed")));
        assert_eq!(prv.to_vec(), h(v, "prv"), "{}", s(v, "name"));
        assert_eq!(public_key(&prv).to_vec(), h(v, "pub"), "{}", s(v, "name"));
        ids.insert(s(v, "name").to_owned(), (prv, arr::<32>(&h(v, "pub"))));
    }
    for v in entries(&t, "ecdh") {
        let (a_prv, a_pub) = ids[s(v, "a")];
        let (b_prv, b_pub) = ids[s(v, "b")];
        let secret = h(v, "secret");
        assert_eq!(shared_secret(&a_prv, &b_pub).unwrap().to_vec(), secret);
        assert_eq!(shared_secret(&b_prv, &a_pub).unwrap().to_vec(), secret);
    }
}

/// Adverts are signed by the Python oracle with MeshCore's expanded keys: Ed25519 is
/// deterministic, so our signatures must match byte for byte.
#[test]
fn advert_signatures() {
    let t = load("air.toml");
    let ids: std::collections::HashMap<String, [u8; 64]> = entries(&t, "identity")
        .iter()
        .map(|v| (s(v, "name").to_owned(), arr::<64>(&h(v, "prv"))))
        .collect();
    let packets: std::collections::HashMap<String, Vec<u8>> = entries(&t, "packet")
        .iter()
        .map(|v| (s(v, "name").to_owned(), h(v, "raw")))
        .collect();
    let mut n = 0;
    for v in entries(&t, "advert") {
        let prv = ids[s(v, "identity")];
        let raw = &packets[s(v, "packet")];
        let p = Packet::decode(raw).unwrap();
        let (key, rest) = p.payload.split_at(32);
        let (ts, rest) = rest.split_at(4);
        let (sig, app) = rest.split_at(64);
        let sig: [u8; 64] = sig.try_into().unwrap();
        assert_eq!(lc_meshcore::ecdh::sign(&prv, &[key, ts, app]).unwrap(), sig);
        let key: [u8; 32] = key.try_into().unwrap();
        assert!(lc_meshcore::ecdh::verify(
            &key,
            &[key.as_slice(), ts, app],
            &sig
        ));
        assert!(!lc_meshcore::ecdh::verify(
            &key,
            &[key.as_slice(), ts, b"x"],
            &sig
        ));
        n += 1;
    }
    assert_eq!(n, 3);
}

#[test]
fn packets_round_trip() {
    let t = load("air.toml");
    for v in entries(&t, "packet") {
        let raw = h(v, "raw");
        let p = Packet::decode(&raw).unwrap_or_else(|e| panic!("{}: {e:?}", s(v, "name")));
        assert_eq!(p.route as i64, v["route"].as_integer().unwrap());
        assert_eq!(p.ptype as i64, v["ptype"].as_integer().unwrap());
        assert_eq!(
            i64::from(p.path.len_byte()),
            v["path_len"].as_integer().unwrap()
        );
        assert_eq!(p.path.bytes(), h(v, "path").as_slice());
        let mut out = [0u8; 255];
        let n = p.encode(&mut out).unwrap();
        assert_eq!(&out[..n], raw.as_slice(), "{}", s(v, "name"));
    }
}

#[test]
fn packets_rejected() {
    let t = load("air.toml");
    for v in entries(&t, "reject") {
        assert!(Packet::decode(&h(v, "raw")).is_err(), "{}", s(v, "name"));
    }
}

fn command_for<'a>(name: &str, frame: &'a [u8]) -> Option<Command<'a>> {
    let key = || -> &'a [u8; 32] { frame[frame.len() - 32..].try_into().unwrap() };
    Some(match name {
        "device-query" => Command::DeviceQuery { app_ver: 3 },
        "app-start" => Command::AppStart { name: b"ln" },
        "app-start-no-name" => Command::AppStart { name: b"" },
        "set-time" => Command::SetDeviceTime {
            time: 1_793_000_000,
        },
        "get-contacts" => Command::GetContacts { since: None },
        "get-contacts-since" => Command::GetContacts { since: Some(5) },
        "reset-path" => Command::ResetPath { key: key() },
        "advert-flood" => Command::SendSelfAdvert { flood: true },
        "advert-zero-hop" => Command::SendSelfAdvert { flood: false },
        "get-device-time" => Command::GetDeviceTime,
        "export-private-key" => Command::ExportPrivateKey,
        "get-batt-and-storage" => Command::GetBattAndStorage,
        "get-contact-by-key" => Command::GetContactByKey { key: key() },
        "path-discovery" => Command::SendPathDiscoveryReq { key: key() },
        "send-raw-data" => Command::SendRawData {
            path: Path::new(0x02, &[0xaa, 0xbb]).unwrap(),
            payload: &[1, 2, 3, 4, 5, 6],
        },
        "send-raw-data-zero-hop" => Command::SendRawData {
            path: Path::EMPTY,
            payload: &[1, 2, 3, 4],
        },
        "send-raw-data-hash-2" => Command::SendRawData {
            path: Path::new(0x41, &[0xaa, 0xbb]).unwrap(),
            payload: &[1, 2, 3, 4],
        },
        "send-raw-packet" => Command::SendRawPacket {
            priority: 1,
            raw: &frame[2..],
        },
        "txt-msg" => Command::SendTxtMsg {
            kind: 0,
            attempt: 2,
            ts: 1_793_000_000,
            prefix: frame[7..13].try_into().unwrap(),
            text: &frame[13..],
        },
        "channel-txt" => Command::SendChannelTxtMsg {
            kind: 0,
            channel: 1,
            ts: 1_793_000_000,
            text: b"hello",
        },
        "channel-txt-empty" => Command::SendChannelTxtMsg {
            kind: 0,
            channel: 0,
            ts: 1_793_000_000,
            text: b"",
        },
        "set-advert-name" => Command::SetAdvertName { name: b"oxymesh" },
        "sync" => Command::SyncNextMessage,
        "get-channel" => Command::GetChannel { index: 39 },
        "set-channel" => Command::SetChannel {
            index: 39,
            name: b"ln-abcd",
            secret: frame[34..50].try_into().unwrap(),
        },
        "set-channel-clear" => Command::SetChannel {
            index: 38,
            name: b"",
            secret: &[0; 16],
        },
        _ => return None,
    })
}

#[test]
fn commands_match_meshcore_js() {
    let t = load("companion.toml");
    let mut seen = 0;
    for v in entries(&t, "command") {
        let frame = h(v, "hex");
        let Some(cmd) = command_for(s(v, "name"), &frame) else {
            continue;
        };
        let mut out = [0u8; 176];
        let n = cmd.encode(&mut out).unwrap();
        assert_eq!(&out[..n], frame.as_slice(), "{}", s(v, "name"));
        seen += 1;
    }
    assert_eq!(seen, 26);
}

#[test]
fn responses_decode() {
    let t = load("companion.toml");
    for v in entries(&t, "response") {
        let frame = h(v, "hex");
        let name = s(v, "name");
        let r = Response::decode(&frame);
        if s(v, "expect") == "ok" {
            assert!(r.is_ok(), "{name}: {r:?}");
        }
        match (name, r) {
            (
                "raw-data",
                Ok(Response::RawData {
                    snr4,
                    rssi,
                    payload,
                }),
            ) => {
                assert_eq!((snr4, rssi, payload), (40, -75, &[1u8, 2, 3, 4, 5][..]));
            }
            ("log-rx-data", Ok(Response::LogRxData { snr4, rssi, raw })) => {
                assert_eq!((snr4, rssi, raw), (44, -80, &[0x11u8, 0x22][..]));
            }
            ("contact", Ok(Response::Contact(c))) => assert_eq!(c.key[0], 0x80),
            ("self-info", Ok(Response::SelfInfo(si))) => {
                assert_eq!((si.tx_power_dbm, si.max_tx_power_dbm), (22, 22));
                assert_eq!(si.key[0], 0x10);
            }
            (
                "sent",
                Ok(Response::Sent {
                    flood,
                    tag,
                    timeout_ms,
                }),
            ) => {
                assert_eq!((flood, tag, timeout_ms), (false, 0xefbe_adde, 5000));
            }
            (
                "batt-and-storage",
                Ok(Response::BattAndStorage {
                    millivolts,
                    used_kb,
                    total_kb,
                }),
            ) => {
                assert_eq!(
                    (millivolts, used_kb, total_kb),
                    (4123, Some(12), Some(1408))
                );
            }
            ("device-info", Ok(Response::DeviceInfo(d))) => {
                assert_eq!((d.fw_ver, d.max_channels), (14, Some(40)));
            }
            ("path-discovery-response", Ok(Response::PathDiscoveryResponse { out_path, .. })) => {
                assert_eq!(
                    (out_path.len_byte(), out_path.bytes()),
                    (2, &[0x11u8, 0x22][..])
                );
            }
            ("private-key", Ok(r)) => assert!(matches!(r, Response::PrivateKey(_))),
            ("disabled", Ok(r)) => assert_eq!(r, Response::Disabled),
            ("err", Ok(r)) => assert_eq!(r, Response::Err(2)),
            (
                "contact-msg-plain",
                Ok(Response::ContactMsg {
                    snr4,
                    prefix,
                    path_len,
                    kind,
                    ts,
                    text,
                }),
            ) => {
                assert_eq!((snr4, prefix[0], path_len, kind), (Some(40), 0x80, 255, 0));
                assert_eq!((ts, text), (1_793_000_000, &b"hello"[..]));
            }
            ("contact-msg", Ok(Response::ContactMsg { snr4, path_len, .. })) => {
                assert_eq!((snr4, path_len), (Some(-20), 3));
            }
            ("contact-msg-legacy", Ok(Response::ContactMsg { snr4, text, .. })) => {
                assert_eq!((snr4, text), (None, &b"hello"[..]));
            }
            (
                "channel-msg",
                Ok(Response::ChannelMsg {
                    snr4,
                    channel,
                    path_len,
                    kind,
                    ts,
                    text,
                }),
            ) => {
                assert_eq!((snr4, channel, path_len, kind), (Some(20), 0, 1, 0));
                assert_eq!((ts, text), (1_793_000_000, &b"alice: hi"[..]));
            }
            (
                "channel-msg-legacy",
                Ok(Response::ChannelMsg {
                    snr4,
                    channel,
                    text,
                    ..
                }),
            ) => {
                assert_eq!((snr4, channel, text), (None, 2, &b"bob: yo"[..]));
            }
            (
                "channel-info",
                Ok(Response::ChannelInfo {
                    index,
                    name,
                    secret,
                }),
            ) => {
                assert_eq!((index, name, secret[0]), (39, &b"ln-abcd"[..], 0x40));
            }
            (
                "channel-info-empty",
                Ok(Response::ChannelInfo {
                    index,
                    name,
                    secret,
                }),
            ) => {
                assert_eq!((index, name, secret), (5, &b""[..], &[0u8; 16]));
            }
            ("channel-info-short", r) => assert!(r.is_err()),
            _ => {}
        }
    }
}

#[test]
fn transport_codes() {
    let t = load("air.toml");
    for v in entries(&t, "transport") {
        let key = lc_meshcore::air::region_key(s(v, "region"));
        let ptype = match v["ptype"].as_integer().unwrap() {
            4 => lc_meshcore::PayloadType::Advert,
            5 => lc_meshcore::PayloadType::GrpTxt,
            6 => lc_meshcore::PayloadType::GrpData,
            p => panic!("ptype {p}"),
        };
        let code = lc_meshcore::air::transport_code(&key, ptype, &h(v, "payload"));
        assert_eq!(
            i64::from(code),
            v["code"].as_integer().unwrap(),
            "{}",
            s(v, "region")
        );
    }
}

#[test]
fn default_flood_scope() {
    let t = load("companion.toml");
    let mut seen = 0;
    for v in entries(&t, "response") {
        let frame = h(v, "hex");
        if frame.first() != Some(&28) {
            continue;
        }
        match Response::decode(&frame).unwrap() {
            Response::DefaultFloodScope {
                name,
                key: Some(key),
            } => {
                assert_eq!(name, b"fr");
                assert_eq!(key, &lc_meshcore::air::region_key("fr"));
            }
            Response::DefaultFloodScope { key: None, .. } => {}
            r => panic!("{r:?}"),
        }
        seen += 1;
    }
    assert!(seen >= 1);
    let mut out = [0u8; 4];
    assert_eq!(Command::GetDefaultFloodScope.encode(&mut out), Ok(1));
    assert_eq!(out[0], 64);
}

#[test]
fn route_has_transport() {
    assert!(Route::TransportFlood.has_transport());
    assert!(!Route::Direct.has_transport());
}

/// Search (§11.2): the radio's noise floor from `GET_STATS` type 1, and its TX power set by
/// POWER (layouts from oxymesh's companion notes, stock MeshCore v1.18 `MyMesh.cpp` 1460, 1914).
#[test]
fn radio_stats_and_tx_power() {
    let mut out = [0u8; 4];
    assert_eq!(Command::GetStats { kind: 1 }.encode(&mut out), Ok(2));
    assert_eq!(&out[..2], &[56, 1]);
    assert_eq!(Command::SetRadioTxPower { dbm: -9 }.encode(&mut out), Ok(2));
    assert_eq!(&out[..2], &[12, 0xF7]);
    // STATS type 1: noise -117 dBm, last RSSI -98, SNR -5.5 dB (×4 = -22), 123 s TX, 4567 s RX.
    let mut f = vec![24, 1];
    f.extend_from_slice(&(-117i16).to_le_bytes());
    f.extend_from_slice(&[(-98i8) as u8, (-22i8) as u8]);
    f.extend_from_slice(&123u32.to_le_bytes());
    f.extend_from_slice(&4567u32.to_le_bytes());
    assert_eq!(
        Response::decode(&f).unwrap(),
        Response::RadioStats {
            noise: -117,
            rssi: -98,
            snr4: -22,
            tx_air_s: 123,
            rx_air_s: 4567
        }
    );
    assert_eq!(
        Response::decode(&[24, 0, 1, 2]).unwrap(),
        Response::OtherStats { kind: 0 }
    );
    assert!(Response::decode(&[24, 1, 0]).is_err(), "truncated");
}

/// ADR-0035: a group voice note's signed message (611 bytes) verifies, in several parts as
/// lichen signs it; one byte changed does not.
#[test]
fn long_messages_verify() {
    let prv = expand_seed(&[7; 32]);
    let key = public_key(&prv);
    let body = vec![0x5Au8; 576];
    let parts: [&[u8]; 5] = [b"lichen/1 group", &[1; 16], &[2], &[3; 4], &body];
    let sig = lc_meshcore::ecdh::sign(&prv, &parts).unwrap();
    assert!(lc_meshcore::ecdh::verify(&key, &parts, &sig));
    let mut other = body.clone();
    other[575] ^= 1;
    let parts: [&[u8]; 5] = [b"lichen/1 group", &[1; 16], &[2], &[3; 4], &other];
    assert!(!lc_meshcore::ecdh::verify(&key, &parts, &sig));
}
