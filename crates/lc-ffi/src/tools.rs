//! Functions that need no core: phrases, GIF links, file names, voice and image preparation, QR codes, USB framing.

use super::*;

/// The seed of a 24-word recovery phrase, to restore an identity (§2).
#[uniffi::export]
pub fn seed_from_phrase(words: Vec<String>) -> Result<Vec<u8>, LichenError> {
    lc_core::identity::seed_from_phrase(&words)
        .map(|s| s.to_vec())
        .ok_or(LichenError::BadCode)
}

/// A GIPHY link turned into a GIF (§6.3): its id and title (from the link's slug, may be
/// empty). None unless `text` is only such a link.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GifLink {
    pub id: String,
    pub title: String,
}

#[uniffi::export]
pub fn gif_link(text: String) -> Option<GifLink> {
    lc_core::content::gif_link(&text).map(|(id, title)| GifLink { id, title })
}

/// `name` fitted to `max_bytes` of UTF-8, its extension kept (ADR-0022): "Very long report
/// title.pdf" becomes "Very long rep….pdf". A name received from someone else is only a label:
/// path separators and control characters are dropped, and so are leading dots and spaces, so
/// that the result is never "." or "..", nor a hidden file. It may be empty: callers name it
/// themselves then. Apps still write a copy into a folder of its own (§13.2).
#[uniffi::export]
pub fn fit_file_name(name: String, max_bytes: u32) -> String {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':') && !c.is_control())
        .collect();
    let name = name
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end()
        .to_owned();
    let max = max_bytes as usize;
    if name.len() <= max {
        return name;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= 10 => name.split_at(dot),
        _ => (name.as_str(), ""),
    };
    let room = max.saturating_sub(ext.len() + '…'.len_utf8());
    let mut end = room.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    let cut = format!("{}…{ext}", &stem[..end]);
    if cut.len() <= max {
        cut
    } else {
        let mut end = max.min(name.len());
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name[..end].to_owned()
    }
}

/// The fingerprint of an identity key (6 groups of 4 hexadecimal digits).
#[uniffi::export]
pub fn fingerprint_of(identity: Vec<u8>) -> Result<Vec<String>, LichenError> {
    let k = key(&identity)?;
    Ok(lc_core::identity::fingerprint(&k).to_vec())
}

/// A voice message ready to send.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PreparedVoice {
    pub mode: u8,
    pub frames: u16,
    pub duration_ms: u32,
    pub data: Vec<u8>,
}

/// Encodes 8 kHz mono samples (at most 15 s are kept) in Codec2 `mode` (§13.1: 0 = 700C,
/// 2 = 1600, 3 = 2400; an unknown mode gives the default, 1600). The speech is prepared first
/// (level, rumble), so the app hands in the microphone's samples as they are.
#[uniffi::export]
pub fn prepare_voice(pcm: Vec<i16>, mode: u8) -> PreparedVoice {
    let mode = lc_media::voice::Mode::from_code(mode).unwrap_or(lc_media::voice::ENCODE_MODE);
    let e = lc_media::voice::encode_mode(&pcm, mode);
    PreparedVoice {
        mode: e.mode.code(),
        frames: e.frames,
        duration_ms: lc_media::voice::duration_ms(e.frames),
        data: e.data,
    }
}

/// 8 kHz mono samples of a voice message; none when this build does not support its mode.
#[uniffi::export]
pub fn decode_voice(mode: u8, frames: u16, data: Vec<u8>) -> Option<Vec<i16>> {
    lc_media::voice::decode(mode, frames, &data)
}

/// Bytes of a VOICE datagram for `voice` (for estimates).
#[uniffi::export]
pub fn voice_datagram_len(voice: &PreparedVoice) -> u32 {
    8 + voice.data.len() as u32
}

/// Bytes of a VOICE datagram of `duration_ms` in `mode` (the cost while recording).
#[uniffi::export]
pub fn voice_datagram_len_for(mode: u8, duration_ms: u32) -> u32 {
    let mode = lc_media::voice::Mode::from_code(mode).unwrap_or(lc_media::voice::ENCODE_MODE);
    let frames = duration_ms
        .div_ceil(lc_media::voice::FRAME_MS)
        .min(u32::from(lc_media::voice::MAX_FRAMES)) as u16;
    8 + lc_media::voice::data_len(mode, frames) as u32
}

/// An image made into a thumbnail: a preview (sent) and a full variant (on request).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PreparedImage {
    pub width: u16,
    pub height: u16,
    pub preview: Vec<u8>,
    pub full: Vec<u8>,
}

/// Thumbnails of an RGBA8 image (`width × height × 4` bytes).
#[uniffi::export]
pub fn prepare_image(rgba: Vec<u8>, width: u32, height: u32) -> Result<PreparedImage, LichenError> {
    let t = lc_media::image::thumbnail(&rgba, width, height).map_err(|_| LichenError::BadImage)?;
    Ok(PreparedImage {
        width: t.width,
        height: t.height,
        preview: t.preview,
        full: t.full,
    })
}

/// Bytes of the FILE datagram carrying `image`'s preview (for estimates).
#[uniffi::export]
pub fn image_datagram_len(image: &PreparedImage) -> u32 {
    21 + image.preview.len() as u32
}

/// UUIDs of the MeshCore companion's Bluetooth service (Nordic UART).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BleUuids {
    pub service: String,
    /// The app writes command frames here.
    pub rx: String,
    /// The companion notifies frames here.
    pub tx: String,
}

#[uniffi::export]
pub fn companion_ble_uuids() -> BleUuids {
    BleUuids {
        service: "6E400001-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        rx: "6E400002-B5A3-F393-E0A9-E50E24DCCA9E".into(),
        tx: "6E400003-B5A3-F393-E0A9-E50E24DCCA9E".into(),
    }
}

/// A QR code's modules, row by row (`width` × `width`, true = dark), error correction M: the
/// contact card's code, drawn by each app (§4.7).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct QrCode {
    pub width: u32,
    pub dark: Vec<bool>,
}

#[uniffi::export]
pub fn qr_code(text: String) -> Result<QrCode, LichenError> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
        .map_err(|_| LichenError::TooLarge)?;
    Ok(QrCode {
        width: u32::try_from(code.width()).unwrap_or(0),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect(),
    })
}

/// A companion frame wrapped for the radio's USB serial port (`'<' ‖ len ‖ frame`), for apps
/// that reach the radio over USB (Android OTG). Over BLE, frames go as they are.
#[uniffi::export]
pub fn usb_encode(frame: Vec<u8>) -> Result<Vec<u8>, LichenError> {
    let mut out = [0u8; lc_meshcore::FRAME_MAX + 3];
    let n = lc_meshcore::usb::encode(&frame, &mut out).map_err(|_| LichenError::TooLarge)?;
    Ok(out.get(..n).unwrap_or(&[]).to_vec())
}

/// Splits the bytes read from the radio's USB serial port (`'>' ‖ len ‖ frame`) into companion
/// frames.
#[derive(uniffi::Object, Default, Debug)]
pub struct UsbDecoder {
    inner: Mutex<lc_meshcore::usb::Decoder>,
}

#[uniffi::export]
impl UsbDecoder {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    /// The frames completed by `bytes`, in order (bytes outside frames are skipped).
    pub fn push(&self, bytes: Vec<u8>) -> Vec<Vec<u8>> {
        let mut d = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut frames = Vec::new();
        for b in bytes {
            if d.push(b) {
                frames.push(d.frame().to_vec());
            }
        }
        frames
    }
}
