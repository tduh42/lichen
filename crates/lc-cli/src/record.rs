//! `lichen search`: a reception recorder for search-mode field tests (protocol.md §11.2, L8c).
//!
//! Every frame the radio hears (`LOG_RX_DATA`) becomes a CSV row with its SNR, its RSSI and the
//! radio's noise floor (`GET_STATS`, every 30 s), where the recorder stands. By default only
//! frames heard directly (0 hops) are kept: a relayed copy measures the last repeater. No lichen
//! session: the companion is only listened to.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lc_meshcore::air::{Packet, PayloadType, Route};
use lc_meshcore::codes::APP_VER;
use lc_meshcore::command::Command;
use lc_meshcore::response::Response;
use lc_meshcore::usb;
use serialport::SerialPort;

const HEADER: &str = "unix_ms,route,type,hops,last_hop,snr_db,rssi_dbm,noise_dbm,len,from,\
                      gid_hash,counter,copy,lat,lon";
const STATS_PERIOD: Duration = Duration::from_secs(30);

pub fn run(
    port: &str,
    csv: Option<&Path>,
    at: Option<(f64, f64)>,
    all: bool,
    minutes: u64,
    verbose: bool,
) -> Result<(), String> {
    let mut out: Box<dyn Write> = match csv {
        Some(p) => {
            let fresh = !p.exists();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .map_err(|e| format!("{}: {e}", p.display()))?;
            if fresh {
                writeln!(f, "{HEADER}").map_err(|e| e.to_string())?;
            }
            Box::new(f)
        }
        None => {
            println!("{HEADER}");
            Box::new(std::io::stdout())
        }
    };
    let mut serial = serialport::new(port, 115_200)
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| format!("{port}: {e}"))?;
    // Opening may reset the board: let it boot, then drop what it printed.
    std::thread::sleep(Duration::from_millis(2_500));
    let _ = serial.clear(serialport::ClearBuffer::Input);
    write_cmd(&mut *serial, &Command::DeviceQuery { app_ver: APP_VER })?;
    write_cmd(
        &mut *serial,
        &Command::AppStart {
            name: b"lichen-recorder",
        },
    )?;
    eprintln!(
        "recording {} frames on {port}{}",
        if all { "all" } else { "direct (0-hop)" },
        csv.map_or(String::new(), |p| format!(" to {}", p.display()))
    );
    let (lat, lon) = at.map_or((String::new(), String::new()), |(a, b)| {
        (format!("{a:.6}"), format!("{b:.6}"))
    });
    let mut decoder = usb::Decoder::new();
    let mut noise: Option<i16> = None;
    let mut rows = 0u64;
    let deadline = Instant::now() + Duration::from_secs(minutes * 60);
    let mut next_stats = Instant::now();
    let mut buf = [0u8; 512];
    while Instant::now() < deadline {
        if Instant::now() >= next_stats {
            next_stats = Instant::now() + STATS_PERIOD;
            write_cmd(&mut *serial, &Command::GetStats { kind: 1 })?;
        }
        let n = match serial.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => 0,
            Err(e) => return Err(format!("read: {e}")),
        };
        for &b in buf.get(..n).unwrap_or(&[]) {
            if !decoder.push(b) {
                continue;
            }
            let frame = decoder.frame();
            if verbose {
                eprintln!("  <- {}", hex::encode(frame));
            }
            match Response::decode(frame) {
                Ok(Response::RadioStats { noise: n, .. }) => noise = Some(n),
                Ok(Response::LogRxData { snr4, rssi, raw }) => {
                    if let Some(r) = row(raw, snr4, rssi, noise, all, &lat, &lon) {
                        writeln!(out, "{r}")
                            .and_then(|()| out.flush())
                            .map_err(|e| e.to_string())?;
                        rows += 1;
                    }
                }
                _ => {}
            }
        }
    }
    eprintln!("{rows} rows");
    Ok(())
}

fn write_cmd(serial: &mut dyn SerialPort, cmd: &Command<'_>) -> Result<(), String> {
    let mut frame = [0u8; lc_meshcore::FRAME_MAX];
    let n = cmd.encode(&mut frame).map_err(|e| format!("{e:?}"))?;
    let mut wire = [0u8; lc_meshcore::FRAME_MAX + 3];
    let m = usb::encode(frame.get(..n).unwrap_or(&[]), &mut wire).map_err(|e| format!("{e:?}"))?;
    serial
        .write_all(wire.get(..m).unwrap_or(&[]))
        .map_err(|e| format!("write: {e}"))
}

/// One CSV row for a heard frame, or `None` (undecodable, or relayed when only direct ones are
/// kept).
fn row(
    raw: &[u8],
    snr4: i8,
    rssi: i8,
    noise: Option<i16>,
    all: bool,
    lat: &str,
    lon: &str,
) -> Option<String> {
    let p = Packet::decode(raw).ok()?;
    let hops = p.path.hops();
    if hops > 0 && !all {
        return None;
    }
    let route = match p.route {
        Route::Flood => "flood",
        Route::TransportFlood => "tflood",
        Route::Direct => "direct",
        Route::TransportDirect => "tdirect",
    };
    let last_hop = p
        .path
        .bytes()
        .last()
        .map_or(String::new(), |b| format!("{b:02x}"));
    // Who sent it, when the frame says: an advert's key, a group envelope's clear header.
    let (from, gid_hash, counter, copy) = match p.ptype {
        PayloadType::Advert => (
            p.payload.get(..4).map_or(String::new(), hex::encode),
            String::new(),
            String::new(),
            String::new(),
        ),
        PayloadType::GrpData => match lc_proto::group::parse(p.payload) {
            Some((h, _)) => (
                format!("{:02x}", h.sender),
                format!("{:04x}", h.gid_hash),
                h.counter.to_string(),
                h.copy.to_string(),
            ),
            None => Default::default(),
        },
        _ => Default::default(),
    };
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Some(format!(
        "{unix_ms},{route},{:?},{hops},{last_hop},{:.2},{},{},{},{from},{gid_hash},{counter},{copy},{lat},{lon}",
        p.ptype,
        f32::from(snr4) / 4.0,
        if rssi == 0 {
            String::new()
        } else {
            rssi.to_string()
        },
        noise.map_or(String::new(), |n| n.to_string()),
        raw.len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lc_meshcore::path::Path as MeshPath;

    fn packet(route: Route, path: &[u8], payload: &[u8]) -> Vec<u8> {
        let p = Packet {
            route,
            ptype: PayloadType::GrpData,
            transport: None,
            path: MeshPath::new(path.len() as u8, path).unwrap(),
            payload,
        };
        let mut raw = [0u8; 255];
        let n = p.encode(&mut raw).unwrap();
        raw[..n].to_vec()
    }

    #[test]
    fn rows() {
        let direct = packet(Route::Flood, &[], &[0u8; 40]);
        let r = row(&direct, -22, 0, Some(-115), false, "45.1", "5.7").unwrap();
        let cols: Vec<&str> = r.split(',').collect();
        assert_eq!(cols.len(), HEADER.split(',').count());
        assert_eq!(
            &cols[1..9],
            ["flood", "GrpData", "0", "", "-5.50", "", "-115", "42"]
        );
        assert_eq!(&cols[13..], ["45.1", "5.7"]);
        // Relayed: dropped unless asked for.
        let relayed = packet(Route::Flood, &[0x42], &[0u8; 40]);
        assert!(row(&relayed, 20, -80, None, false, "", "").is_none());
        let r = row(&relayed, 20, -80, None, true, "", "").unwrap();
        assert!(r.contains(",flood,GrpData,1,42,5.00,-80,,"));
    }
}
