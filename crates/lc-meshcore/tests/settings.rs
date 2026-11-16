//! Radio settings against `testdata/meshcore/settings.toml`, written from the v1.18 firmware's
//! handlers by `settings.py` (an independent oracle: Python's struct).

use lc_meshcore::{Command, OtherParams, Response};
use toml::{Table, Value};

fn load() -> Table {
    let path = format!(
        "{}/../../testdata/meshcore/settings.toml",
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

fn b(v: &Value, k: &str) -> bool {
    v[k].as_bool().unwrap()
}

fn other(v: &Value) -> OtherParams {
    OtherParams {
        manual_add: b(v, "manual_add"),
        telemetry: n(v, "telemetry"),
        advert_loc_policy: n(v, "advert_loc_policy"),
        multi_acks: n(v, "multi_acks"),
    }
}

#[test]
fn settings_commands_match_the_firmware() {
    let t = load();
    let mut seen = 0;
    for v in entries(&t, "command") {
        let name = s(v, "name");
        let cmd = match name {
            "set-radio-params" => Command::SetRadioParams {
                freq_khz: n(v, "freq_khz"),
                bw_hz: n(v, "bw_hz"),
                sf: n(v, "sf"),
                cr: n(v, "cr"),
                repeat: b(v, "repeat"),
            },
            "set-advert-latlon" => Command::SetAdvertLatLon {
                lat_e6: n(v, "lat_e6"),
                lon_e6: n(v, "lon_e6"),
            },
            "reboot" => Command::Reboot,
            "set-device-pin" => Command::SetDevicePin { pin: n(v, "pin") },
            "set-other-params" => Command::SetOtherParams(other(v)),
            "get-custom-vars" => Command::GetCustomVars,
            "set-custom-var" => Command::SetCustomVar {
                name: s(v, "var"),
                value: s(v, "value"),
            },
            "set-autoadd-config" => Command::SetAutoaddConfig {
                mask: n(v, "mask"),
                max_hops: n(v, "max_hops"),
            },
            "get-autoadd-config" => Command::GetAutoaddConfig,
            other => panic!("unknown vector {other}"),
        };
        let mut out = [0u8; 176];
        let len = cmd.encode(&mut out).unwrap();
        assert_eq!(hex::encode(&out[..len]), s(v, "hex"), "{name}");
        seen += 1;
    }
    assert_eq!(seen, 9);
}

#[test]
fn settings_responses_decode() {
    let t = load();
    let mut seen = 0;
    for v in entries(&t, "response") {
        let frame = hex::decode(s(v, "hex")).unwrap();
        let name = s(v, "name");
        match Response::decode(&frame).unwrap() {
            Response::CustomVars(text) => assert_eq!(text, s(v, "text").as_bytes(), "{name}"),
            Response::AutoaddConfig { mask, max_hops } => {
                assert_eq!((mask, max_hops), (n(v, "mask"), n(v, "max_hops")), "{name}");
            }
            Response::SelfInfo(si) => {
                assert_eq!(si.tx_power_dbm, n::<i8>(v, "tx_power"));
                assert_eq!(si.max_tx_power_dbm, n::<i8>(v, "max_tx_power"));
                assert_eq!((si.lat_e6, si.lon_e6), (n(v, "lat_e6"), n(v, "lon_e6")));
                assert_eq!(si.other, other(v));
                assert_eq!((si.freq_khz, si.bw_hz), (n(v, "freq_khz"), n(v, "bw_hz")));
                assert_eq!((si.sf, si.cr), (n(v, "sf"), n(v, "cr")));
                assert_eq!(si.name, s(v, "node_name").as_bytes());
            }
            r => panic!("{name}: {r:?}"),
        }
        seen += 1;
    }
    assert_eq!(seen, 4);
}

/// What the firmware would refuse is not sent: a PIN out of range, a variable that would not
/// parse back.
#[test]
fn invalid_settings_are_not_encoded() {
    let mut out = [0u8; 176];
    for pin in [1, 99_999, 1_000_000] {
        assert!(
            Command::SetDevicePin { pin }.encode(&mut out).is_err(),
            "{pin}"
        );
    }
    for (name, value) in [
        ("", "1"),
        ("gps", ""),
        ("g:ps", "1"),
        ("gps", "1,2"),
        ("gps", "é"),
    ] {
        assert!(
            Command::SetCustomVar { name, value }
                .encode(&mut out)
                .is_err(),
            "{name}:{value}"
        );
    }
    assert_eq!(
        Command::SetAutoaddConfig {
            mask: 1,
            max_hops: 200
        }
        .encode(&mut out),
        Ok(3)
    );
    assert_eq!(out[2], 64, "max hops capped as the firmware does");
}
