//! Mesh tools against `testdata/meshcore/mesh.toml`, written from the v1.18 firmware's handlers
//! by `mesh.py` (an independent oracle: Python's struct).

use lc_meshcore::command::neighbours_request;
use lc_meshcore::server::{Neighbours, ServerStatus};
use lc_meshcore::{Command, Response};
use toml::{Table, Value};

fn load() -> Table {
    let path = format!(
        "{}/../../testdata/meshcore/mesh.toml",
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

fn n<T: TryFrom<i64>>(v: &Value, k: &str) -> T
where
    T::Error: std::fmt::Debug,
{
    T::try_from(v[k].as_integer().unwrap()).unwrap()
}

fn bytes(v: &Value, k: &str) -> Vec<u8> {
    hex::decode(s(v, k)).unwrap()
}

fn i8s(v: &Value, k: &str) -> Vec<u8> {
    v[k].as_array()
        .unwrap()
        .iter()
        .map(|x| i8::try_from(x.as_integer().unwrap()).unwrap() as u8)
        .collect()
}

#[test]
fn mesh_commands_match_the_firmware() {
    let t = load();
    let mut seen = 0;
    for v in entries(&t, "command") {
        let name = s(v, "name");
        let path = v.get("path").map(|_| bytes(v, "path"));
        let key: Option<[u8; 32]> = v.get("key").map(|_| bytes(v, "key").try_into().unwrap());
        let prefix: Option<[u8; 6]> = v
            .get("prefix")
            .map(|_| bytes(v, "prefix").try_into().unwrap());
        let random: Option<[u8; 4]> = v
            .get("random")
            .map(|_| bytes(v, "random").try_into().unwrap());
        let req = random.map(|r| {
            neighbours_request(
                n(v, "count"),
                n(v, "offset"),
                n(v, "order"),
                n(v, "prefix_len"),
                r,
            )
        });
        let cmd = match name {
            "send-login" => Command::SendLogin {
                key: key.as_ref().unwrap(),
                password: s(v, "password").as_bytes(),
            },
            "send-status-req" => Command::SendStatusReq {
                key: key.as_ref().unwrap(),
            },
            "logout" => Command::Logout {
                key: key.as_ref().unwrap(),
            },
            "send-telemetry-req" => Command::SendTelemetryReq {
                key: key.as_ref().unwrap(),
            },
            "send-binary-req-neighbours" => Command::SendBinaryReq {
                key: key.as_ref().unwrap(),
                data: req.as_ref().unwrap(),
            },
            "send-txt-msg-cli" => Command::SendTxtMsg {
                kind: n(v, "kind"),
                attempt: n(v, "attempt"),
                ts: n(v, "ts"),
                prefix: prefix.as_ref().unwrap(),
                text: s(v, "text").as_bytes(),
            },
            "send-trace-path" | "send-trace-path-2-byte" => Command::SendTracePath {
                tag: n(v, "tag"),
                auth: n(v, "auth"),
                flags: n(v, "flags"),
                path: path.as_deref().unwrap(),
            },
            other => panic!("unknown vector {other}"),
        };
        let mut out = [0u8; 176];
        let len = cmd.encode(&mut out).unwrap();
        assert_eq!(hex::encode(&out[..len]), s(v, "hex"), "{name}");
        seen += 1;
    }
    assert_eq!(seen, 8);
}

#[test]
fn mesh_responses_decode() {
    let t = load();
    let mut seen = 0;
    for v in entries(&t, "response") {
        let frame = hex::decode(s(v, "hex")).unwrap();
        let name = s(v, "name");
        match Response::decode(&frame).unwrap() {
            Response::TraceData {
                flags,
                tag,
                auth,
                hashes,
                snrs,
                final_snr4,
            } => {
                assert_eq!(flags, n::<u8>(v, "flags"), "{name}");
                assert_eq!((tag, auth), (n(v, "tag"), n(v, "auth")), "{name}");
                assert_eq!(hashes, bytes(v, "hashes"), "{name}");
                assert_eq!(snrs, i8s(v, "snrs"), "{name}");
                assert_eq!(final_snr4, n::<i8>(v, "final_snr4"), "{name}");
            }
            Response::LoginSuccess {
                admin,
                prefix,
                server_time,
                permissions,
                fw_level,
            } => {
                assert_eq!(admin, n::<u8>(v, "admin"), "{name}");
                assert_eq!(prefix[..], bytes(v, "prefix"), "{name}");
                let opt = |k: &str| v.get(k).map(|x| x.as_integer().unwrap());
                assert_eq!(server_time.map(i64::from), opt("server_time"), "{name}");
                assert_eq!(permissions.map(i64::from), opt("permissions"), "{name}");
                assert_eq!(fw_level.map(i64::from), opt("fw_level"), "{name}");
            }
            Response::LoginFail { prefix } => assert_eq!(prefix[..], bytes(v, "prefix")),
            Response::StatusResponse { prefix, data } => {
                assert_eq!(prefix[..], bytes(v, "prefix"), "{name}");
                let st = ServerStatus::decode(data).unwrap();
                assert_eq!(st.battery_mv, n::<u16>(v, "battery_mv"), "{name}");
                assert_eq!(st.uptime_s, n::<u32>(v, "uptime_s"), "{name}");
                assert_eq!(st.last_snr4, n::<i16>(v, "last_snr4"), "{name}");
                if name == "status-response-repeater" {
                    assert_eq!(st.tx_queue, n::<u16>(v, "tx_queue"));
                    assert_eq!(st.noise_floor, n::<i16>(v, "noise_floor"));
                    assert_eq!(st.last_rssi, n::<i16>(v, "last_rssi"));
                    assert_eq!(st.packets_recv, n::<u32>(v, "packets_recv"));
                    assert_eq!(st.packets_sent, n::<u32>(v, "packets_sent"));
                    assert_eq!(st.air_time_s, n::<u32>(v, "air_time_s"));
                    assert_eq!(st.sent_flood, n::<u32>(v, "sent_flood"));
                    assert_eq!(st.sent_direct, n::<u32>(v, "sent_direct"));
                    assert_eq!(st.recv_flood, n::<u32>(v, "recv_flood"));
                    assert_eq!(st.recv_direct, n::<u32>(v, "recv_direct"));
                    assert_eq!(st.err_events, n::<u16>(v, "err_events"));
                    assert_eq!(st.direct_dups, n::<u16>(v, "direct_dups"));
                    assert_eq!(st.flood_dups, n::<u16>(v, "flood_dups"));
                    assert_eq!(st.rx_air_time_s, Some(n(v, "rx_air_time_s")));
                    assert_eq!(st.recv_errors, Some(n(v, "recv_errors")));
                    assert_eq!(st.posted, None);
                } else {
                    assert_eq!(st.posted, Some(n(v, "posted")));
                    assert_eq!(st.post_pushes, Some(n(v, "post_pushes")));
                    assert_eq!(st.rx_air_time_s, None);
                }
            }
            Response::BinaryResponse { tag, data } => {
                assert_eq!(tag, n::<u32>(v, "tag"), "{name}");
                let nb = Neighbours::decode(data, 6).unwrap();
                assert_eq!((nb.total, nb.count), (n(v, "total"), n(v, "count")));
                let got: Vec<_> = nb.iter().collect();
                let prefixes = bytes(v, "prefixes");
                let ago = v["heard_ago_s"].as_array().unwrap();
                for (i, e) in got.iter().enumerate() {
                    assert_eq!(e.prefix, &prefixes[i * 6..i * 6 + 6]);
                    assert_eq!(i64::from(e.heard_ago_s), ago[i].as_integer().unwrap());
                    assert_eq!(e.snr4 as u8, i8s(v, "snrs")[i]);
                }
                assert_eq!(got.len(), 2);
                // Asked with another prefix length, the entries do not add up.
                assert!(Neighbours::decode(data, 4).is_none());
            }
            Response::ContactMsg {
                prefix,
                path_len,
                kind,
                ts,
                text,
                ..
            } => {
                assert_eq!(prefix[..], bytes(v, "prefix"), "{name}");
                assert_eq!((path_len, kind), (n(v, "path_len"), n(v, "kind")), "{name}");
                assert_eq!(ts, n::<u32>(v, "ts"), "{name}");
                let mut want = v
                    .get("author")
                    .map(|_| bytes(v, "author"))
                    .unwrap_or_default();
                want.extend_from_slice(s(v, "text").as_bytes());
                assert_eq!(text, want, "{name}");
            }
            Response::TelemetryResponse { prefix, data } => {
                assert_eq!(prefix[..], bytes(v, "prefix"), "{name}");
                assert_eq!(data, bytes(v, "data"), "{name}");
                let items: Vec<_> = lc_meshcore::lpp::items(data).collect();
                assert_eq!(items.len(), 2, "{name}: {items:?}");
            }
            r => panic!("{name}: {r:?}"),
        }
        seen += 1;
    }
    assert_eq!(seen, 12);
}

/// Status and neighbours decoders never read past what they were given.
#[test]
fn short_answers_are_refused() {
    assert!(ServerStatus::decode(&[0; 47]).is_none());
    let st = ServerStatus::decode(&[0; 48]).unwrap();
    assert_eq!((st.posted, st.rx_air_time_s), (None, None));
    assert!(Neighbours::decode(&[1, 0, 1, 0, 9, 9], 6).is_none());
    assert!(Neighbours::decode(&[1, 0], 6).is_none());
    let empty = Neighbours::decode(&[3, 0, 0, 0], 6).unwrap();
    assert_eq!((empty.total, empty.iter().count()), (3, 0));
    let mut out = [0u8; 176];
    let long = Command::SendLogin {
        key: &[1; 32],
        password: b"0123456789abcdef",
    };
    assert!(
        long.encode(&mut out).is_err(),
        "16 bytes: the server keeps 15"
    );
    let empty = Command::SendBinaryReq {
        key: &[1; 32],
        data: &[],
    };
    assert!(empty.encode(&mut out).is_err());
}

/// What the firmware refuses is not sent: an empty path, one that is not a whole number of
/// hashes, or one longer than it takes.
#[test]
fn invalid_traces_are_not_encoded() {
    let mut out = [0u8; 176];
    let trace = |flags, path| Command::SendTracePath {
        tag: 1,
        auth: 0,
        flags,
        path,
    };
    assert!(trace(0, &[]).encode(&mut out).is_err());
    assert!(trace(1, &[1, 2, 3]).encode(&mut out).is_err());
    assert!(trace(0, &[7; 65]).encode(&mut out).is_err(), "65 hops");
    assert!(trace(1, &[7; 130]).encode(&mut out).is_err(), "65 hops");
    assert!(trace(0, &[7; 64]).encode(&mut out).is_ok());
    // A truncated push is refused, not read past its end.
    let frame = hex::decode("8900030078563412000000005a775a28f416").unwrap();
    assert!(Response::decode(&frame).is_err());
}
