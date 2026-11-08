//! `lichen bridge`: the radio's USB serial port relayed to a TCP port, byte for byte, so that an
//! app with a TCP link reaches a USB radio. The Android emulator cannot use the Mac's USB devices;
//! it reaches the Mac's loopback at 10.0.2.2. One client at a time: a new one replaces the last.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Client = Arc<Mutex<Option<TcpStream>>>;

pub fn run(port: &str, listen: &str, verbose: bool) -> Result<(), String> {
    let mut serial = serialport::new(port, 115_200)
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| format!("{port}: {e}"))?;
    // Opening may reset the board: let it boot, then drop what it printed.
    std::thread::sleep(Duration::from_millis(2_500));
    let _ = serial.clear(serialport::ClearBuffer::Input);
    let mut to_radio = serial.try_clone().map_err(|e| format!("{port}: {e}"))?;
    let listener = TcpListener::bind(listen).map_err(|e| format!("{listen}: {e}"))?;
    let tcp_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    println!("relaying {port} on {listen} (from the Android emulator: 10.0.2.2:{tcp_port})");

    let client: Client = Arc::new(Mutex::new(None));
    // Radio → client.
    {
        let client = Arc::clone(&client);
        std::thread::spawn(move || {
            let mut buf = [0u8; 512];
            loop {
                match serial.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let chunk = buf.get(..n).unwrap_or(&[]);
                        if verbose {
                            println!("  <- {}", hex::encode(chunk));
                        }
                        let mut guard = client
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if let Some(c) = guard.as_mut()
                            && c.write_all(chunk).is_err()
                        {
                            *guard = None;
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        eprintln!("error: radio read: {e}");
                        std::process::exit(1);
                    }
                }
            }
        });
    }
    // Clients → radio, one at a time.
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        println!("client {peer} connected");
        let _ = stream.set_nodelay(true);
        let Ok(writer) = stream.try_clone() else {
            continue;
        };
        *client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(writer);
        let mut buf = [0u8; 512];
        loop {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = buf.get(..n).unwrap_or(&[]);
                    if verbose {
                        println!("  -> {}", hex::encode(chunk));
                    }
                    to_radio
                        .write_all(chunk)
                        .map_err(|e| format!("radio write: {e}"))?;
                }
            }
        }
        println!("client {peer} gone");
    }
    Ok(())
}
