//! `lichen contacts`: the companion's contact table as it comes, record by record, to see what
//! the core keeps and what it drops. Read only: it sends `APP_START`, `DEVICE_QUERY` and
//! `GET_CONTACTS`, nothing else (no message sync, no clock, no channel scan).

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use lc_meshcore::codes::{APP_VER, resp};
use lc_meshcore::{Command, Response, usb};

/// How long the companion may stay silent before the read is given up.
const SILENCE: Duration = Duration::from_secs(10);

/// Offsets in a `CONTACT` frame (protocol.md §1): code, key (32), type, flags, path length,
/// path (64), name (32), last advert, latitude, longitude, last change.
const KIND: usize = 33;
const FLAGS: usize = 34;
const PATH_LEN: usize = 35;
const NAME: usize = 100;
const RECORD: usize = 148;

pub fn run(port: &str, verbose: bool) -> Result<(), String> {
    let mut port = serialport::new(port, 115_200)
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| format!("{port}: {e}"))?;
    // Opening may reset the board through the USB-serial bridge: let it boot.
    std::thread::sleep(Duration::from_millis(2_500));
    let _ = port.clear(serialport::ClearBuffer::Input);

    let mut send = |cmd: Command<'_>| -> Result<(), String> {
        let mut frame = [0u8; lc_meshcore::FRAME_MAX];
        let n = cmd.encode(&mut frame).map_err(|e| format!("{e:?}"))?;
        let mut out = [0u8; lc_meshcore::FRAME_MAX + 3];
        let m = usb::encode(&frame[..n], &mut out).map_err(|e| format!("{e:?}"))?;
        port.write_all(&out[..m]).map_err(|e| format!("write: {e}"))
    };
    send(Command::AppStart {
        name: b"lichen-cli",
    })?;
    send(Command::DeviceQuery { app_ver: APP_VER })?;
    send(Command::GetContacts { since: None })?;

    let mut decoder = usb::Decoder::new();
    let mut buf = [0u8; 512];
    let mut last = Instant::now();
    let mut announced: Option<u32> = None;
    let (mut records, mut unreadable, mut people) = (0u32, 0u32, 0u32);
    let mut ended = false;
    println!("  #  key       type flags path len  name");
    while !ended && last.elapsed() < SILENCE {
        let n = match port.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(format!("read: {e}")),
        };
        for &b in &buf[..n] {
            if !decoder.push(b) {
                continue;
            }
            last = Instant::now();
            let frame = decoder.frame();
            if verbose {
                eprintln!("  <- {}", hex::encode(frame));
            }
            match frame.first().copied() {
                Some(resp::CONTACT) => {
                    records += 1;
                    let byte = |i: usize| frame.get(i).copied().unwrap_or(0);
                    let name = frame
                        .get(NAME..(NAME + 32).min(frame.len()))
                        .map(|n| String::from_utf8_lossy(n).trim_end_matches('\0').to_owned())
                        .unwrap_or_default();
                    let key = frame.get(1..7).map(hex::encode).unwrap_or_default();
                    let verdict = match Response::decode(frame) {
                        Ok(Response::Contact(c)) => {
                            if matches!(c.kind, 0 | 1) {
                                people += 1;
                                "person".to_owned()
                            } else {
                                "node".to_owned()
                            }
                        }
                        Ok(other) => format!("decoded as {other:?}"),
                        Err(e) => {
                            unreadable += 1;
                            format!("UNREADABLE ({e:?})")
                        }
                    };
                    let size = if frame.len() == RECORD {
                        String::new()
                    } else {
                        format!(" ({} bytes, not {RECORD})", frame.len())
                    };
                    println!(
                        "{records:>3}  {key}  {:>3}  0x{:02x}  0x{:02x}  {name:<32} {verdict}{size}",
                        byte(KIND),
                        byte(FLAGS),
                        byte(PATH_LEN),
                    );
                }
                _ => match Response::decode(frame) {
                    Ok(Response::ContactsStart { count }) => {
                        announced = Some(count);
                        println!("CONTACTS_START: {count} contacts announced");
                    }
                    Ok(Response::EndOfContacts { .. }) => ended = true,
                    Ok(Response::SelfInfo(si)) => {
                        println!("radio: {}", String::from_utf8_lossy(si.name));
                    }
                    Ok(Response::DeviceInfo(d)) => println!(
                        "firmware: {} {} ({}), protocol {}",
                        String::from_utf8_lossy(d.manufacturer),
                        String::from_utf8_lossy(d.version),
                        String::from_utf8_lossy(d.build_date),
                        d.fw_ver
                    ),
                    Ok(_) | Err(_) => {}
                },
            }
        }
    }
    println!();
    println!(
        "{} announced, {records} records received, {unreadable} unreadable, {people} people{}",
        announced.map_or_else(|| "none".to_owned(), |n| n.to_string()),
        if ended {
            ", END_OF_CONTACTS received"
        } else {
            ", NO END_OF_CONTACTS (silent for 10 s)"
        }
    );
    Ok(())
}
