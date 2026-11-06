//! Media over LoRa (protocol.md §13, ADR-0012): voice in Codec2, images as AVIF thumbnails.
//!
//! Pure functions, no IO: PCM samples and RGBA pixels in, bytes out (and back for voice; the
//! systems decode AVIF themselves).

mod codec2_c;
pub mod image;
pub mod voice;
