use lc_meshcore::airtime::LoRaParams;
use lc_meshcore::command::raw_payload_max;
use lc_meshcore::path::MAX_PATH_BYTES;
use lc_meshcore::{Command, Packet, Path, Response, usb};
use proptest::prelude::*;

fn lora(sf: u8, bw_hz: u32, cr: u8, preamble: u16) -> LoRaParams {
    LoRaParams {
        sf,
        bw_hz,
        cr,
        preamble,
    }
}

#[test]
fn airtime_matches_oxymesh_and_datasheet() {
    // Values of oxymesh's mc-core (preamble 8, BW 125 kHz, CR 4/5, 20 bytes).
    assert_eq!(lora(7, 125_000, 5, 8).airtime_us(20), 56_576);
    assert!(lora(12, 125_000, 5, 8).ldro());
    assert_eq!(lora(12, 125_000, 5, 8).airtime_us(20), 1_318_912);
    assert_eq!(lora(5, 125_000, 5, 8).airtime_us(20), 17_216);
    assert_eq!(lora(6, 125_000, 5, 8).airtime_us(20), 31_872);
}

#[test]
fn airtime_meshcore_eu_narrow() {
    // 869.618 MHz / 62.5 kHz / SF8, preamble 32, full 255-byte frame (docs/spec/protocol.md §9).
    let cr5 = LoRaParams::meshcore(8, 62_500, 5).unwrap();
    assert_eq!(cr5.preamble, 32);
    assert!(!cr5.ldro());
    assert_eq!(cr5.airtime_us(255), 1_512_448);
    let cr8 = LoRaParams::meshcore(8, 62_500, 8).unwrap();
    assert_eq!(cr8.airtime_us(255), 2_311_168);
    assert!(LoRaParams::meshcore(4, 62_500, 5).is_none());
    assert!(LoRaParams::meshcore(8, 62_500, 9).is_none());
}

#[test]
fn raw_payload_limits() {
    assert_eq!(raw_payload_max(0), 172);
    assert_eq!(raw_payload_max(2), 172);
    assert_eq!(raw_payload_max(3), 171);
    assert_eq!(raw_payload_max(64), 110);
    let path = Path::from_hashes(&[1, 2, 3]).unwrap();
    let ok = [0u8; 171];
    let mut out = [0u8; 176];
    assert_eq!(
        Command::SendRawData { path, payload: &ok }.encode(&mut out),
        Ok(176)
    );
    let too_long = [0u8; 172];
    assert!(
        Command::SendRawData {
            path,
            payload: &too_long
        }
        .encode(&mut out)
        .is_err()
    );
    assert!(
        Command::SendRawData {
            path,
            payload: &[1, 2, 3]
        }
        .encode(&mut out)
        .is_err()
    );
}

#[test]
fn path_reversal() {
    let p = Path::from_hashes(&[0xa1, 0xb2, 0xc3]).unwrap();
    let mut buf = [0u8; MAX_PATH_BYTES];
    let r = p.reversed(&mut buf).unwrap();
    assert_eq!(r.bytes(), &[0xc3, 0xb2, 0xa1]);
    assert_eq!(r.hops(), 3);
    let p2 = Path::new(0x42, &[1, 2, 3, 4]).unwrap();
    let mut buf = [0u8; MAX_PATH_BYTES];
    assert_eq!(p2.reversed(&mut buf).unwrap().bytes(), &[3, 4, 1, 2]);
    assert!(Path::new(0xC1, &[0; 4]).is_err());
    assert!(Path::new(0x02, &[0; 3]).is_err());
}

#[test]
fn usb_round_trip_with_noise() {
    let mut wire = vec![0x00, 0x13, b'x'];
    let frames: [&[u8]; 3] = [&[0x00], &[0x84, 0x28, 0xb5, 0xff, 1, 2, 3, 4], &[5; 176]];
    for f in frames {
        wire.push(b'>');
        wire.extend_from_slice(&(f.len() as u16).to_le_bytes());
        wire.extend_from_slice(f);
    }
    // A 200-byte frame is read and dropped; a zero length resets the reader.
    wire.extend_from_slice(&[b'>', 200, 0]);
    wire.extend_from_slice(&[9; 200]);
    wire.extend_from_slice(&[b'>', 0, 0, b'>', 1, 0, 0x0a]);
    let mut d = usb::Decoder::new();
    let mut got = Vec::new();
    for b in wire {
        if d.push(b) {
            got.push(d.frame().to_vec());
        }
    }
    assert_eq!(
        got,
        vec![
            vec![0x00],
            vec![0x84, 0x28, 0xb5, 0xff, 1, 2, 3, 4],
            vec![5; 176],
            vec![0x0a]
        ]
    );
    let mut out = [0u8; 179];
    assert_eq!(usb::encode(&[0x16, 0x03], &mut out), Ok(5));
    assert_eq!(&out[..5], &[b'<', 2, 0, 0x16, 0x03]);
    assert!(usb::encode(&[0; 177], &mut out).is_err());
}

proptest! {
    #[test]
    fn response_decode_never_panics(frame in proptest::collection::vec(any::<u8>(), 0..200)) {
        let _ = Response::decode(&frame);
    }

    #[test]
    fn packet_decode_encode_is_identity(raw in proptest::collection::vec(any::<u8>(), 0..300)) {
        if let Ok(p) = Packet::decode(&raw) {
            let mut out = [0u8; 255];
            let n = p.encode(&mut out).unwrap();
            prop_assert_eq!(&out[..n], raw.as_slice());
        }
    }

    #[test]
    fn usb_decoder_never_panics(wire in proptest::collection::vec(any::<u8>(), 0..1000)) {
        let mut d = usb::Decoder::new();
        for b in wire {
            if d.push(b) {
                prop_assert!(!d.frame().is_empty() && d.frame().len() <= 176);
            }
        }
    }
}
