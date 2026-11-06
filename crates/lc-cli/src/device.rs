//! A MeshCore companion on a USB serial port, driven by an `lc_core::Core`.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use lc_core::{Config, Core, Event, Millis};
use lc_meshcore::usb;

pub struct Device {
    port: Box<dyn serialport::SerialPort>,
    decoder: usb::Decoder,
    start: Instant,
    pub core: Core,
    pub verbose: bool,
}

/// What the event handler wants next.
pub enum Flow {
    Continue,
    Stop,
}

impl Device {
    /// Opens the port and starts the connection sequence. Opening may reset the board through
    /// the USB-serial bridge: let it boot, then drop what it printed.
    pub fn open(path: &str) -> Result<Self, String> {
        let port = serialport::new(path, 115_200)
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| format!("{path}: {e}"))?;
        std::thread::sleep(Duration::from_millis(2_500));
        let _ = port.clear(serialport::ClearBuffer::Input);
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| format!("random seed: {e}"))?;
        let mut dev = Self {
            port,
            decoder: usb::Decoder::new(),
            start: Instant::now(),
            core: Core::new(Config::new("lichen-cli", seed)),
            verbose: false,
        };
        let now = dev.now();
        dev.core.connect(now);
        Ok(dev)
    }

    pub fn now(&self) -> Millis {
        self.start.elapsed().as_millis() as Millis
    }

    /// Exchanges frames with the companion and hands every event to `on_event` until it says
    /// stop or `limit` passes. Returns false on timeout.
    pub fn run(
        &mut self,
        limit: Duration,
        mut on_event: impl FnMut(&mut Core, Event, Millis) -> Flow,
    ) -> Result<bool, String> {
        let deadline = Instant::now() + limit;
        let mut buf = [0u8; 512];
        while Instant::now() < deadline {
            let now = self.now();
            while let Some(frame) = self.core.poll_companion(now) {
                if self.verbose {
                    eprintln!("  -> {}", hex::encode(&frame));
                }
                let mut out = [0u8; lc_meshcore::FRAME_MAX + 3];
                let n = usb::encode(&frame, &mut out).map_err(|e| format!("{e:?}"))?;
                self.port
                    .write_all(&out[..n])
                    .map_err(|e| format!("write: {e}"))?;
            }
            match self.port.read(&mut buf) {
                Ok(n) => {
                    for &b in &buf[..n] {
                        if self.decoder.push(b) {
                            if self.verbose {
                                eprintln!("  <- {}", hex::encode(self.decoder.frame()));
                            }
                            let now = self.now();
                            self.core.on_companion_frame(self.decoder.frame(), now);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("read: {e}")),
            }
            let now = self.now();
            self.core.tick(now);
            while let Some(e) = self.core.poll_event() {
                if let Flow::Stop = on_event(&mut self.core, e, now) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
