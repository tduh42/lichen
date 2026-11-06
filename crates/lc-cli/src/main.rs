//! `lichen`: run the lichen core against a MeshCore companion over USB, or on the fake network.

mod device;

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use lc_core::fakenet::Net;
use lc_core::{Content, Core, Event, PubKey};
use lc_meshcore::airtime::LoRaParams;

use device::{Device, Flow};

#[derive(Parser)]
#[command(
    name = "lichen",
    version,
    about = "Encrypted messaging over stock MeshCore"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect to a companion and show its identity, radio, budget and contacts.
    Info {
        /// Serial port of the companion (e.g. /dev/cu.usbmodem2141101).
        port: String,
        /// Print every companion frame.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Send a text or a file to a contact and wait for the acknowledgement.
    Send {
        port: String,
        /// Contact name (case-insensitive) or public-key hex prefix.
        #[arg(long)]
        to: String,
        #[arg(long, conflicts_with = "file")]
        text: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        /// Give up after this many minutes.
        #[arg(long, default_value_t = 30)]
        timeout_min: u64,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Stay connected, answer handshakes and print received messages.
    Listen {
        port: String,
        /// Stop after this many minutes.
        #[arg(long, default_value_t = 600)]
        minutes: u64,
        #[arg(short, long)]
        verbose: bool,
    },
    /// Send a datagram across the fake MeshCore network and print the timeline.
    Sim {
        /// Repeaters between the two companions.
        #[arg(long, default_value_t = 3)]
        repeaters: usize,
        /// Loss per link, in percent.
        #[arg(long, default_value_t = 10)]
        loss: u32,
        /// Datagram size in bytes (1 to 10,048).
        #[arg(long, default_value_t = 2_000)]
        bytes: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Coding rate denominator of the preset (5 = 4/5, 8 = 4/8 as in France).
        #[arg(long, default_value_t = 5)]
        cr: u8,
    },
}

fn unix_now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

fn short(key: &PubKey) -> String {
    hex::encode(&key[..4])
}

fn name_of(core: &Core, key: &PubKey) -> String {
    core.contacts()
        .find(|c| c.key == *key)
        .map_or_else(|| short(key), |c| c.name.clone())
}

fn describe(core: &Core, e: &Event) -> String {
    match e {
        Event::Connected {
            key,
            name,
            freq_khz,
            radio,
        } => format!(
            "connected to \"{name}\" ({}), {}.{:03} MHz / {} Hz / SF{} / CR 4/{}",
            short(key),
            freq_khz / 1000,
            freq_khz % 1000,
            radio.bw_hz,
            radio.sf,
            radio.cr
        ),
        Event::IdentityLearned {
            node,
            identity,
            changed,
        } => format!(
            "{} is identity {}{}",
            name_of(core, node),
            lc_core::identity::fingerprint(identity).join(" "),
            if *changed {
                " (CHANGED: verify again)"
            } else {
                " (to verify)"
            }
        ),
        Event::ContactsUpdated => format!("{} contacts", core.contacts().count()),
        Event::SessionEstablished { peer } => {
            format!("secure session with {}", name_of(core, peer))
        }
        Event::Received {
            from,
            content,
            snr4,
            rssi,
        } => {
            let body = match content {
                Content::Text { text, .. } => format!("\"{text}\""),
                Content::Location(p) => describe_position(p),
                Content::Voice { frames, data, .. } => format!(
                    "voice message, {:.1} s, {} bytes",
                    f32::from(*frames) * 0.04,
                    data.len()
                ),
                Content::Image {
                    full,
                    width,
                    height,
                    data,
                    ..
                } => format!(
                    "image {width}x{height}, {} ({} bytes)",
                    if *full { "full" } else { "preview" },
                    data.len()
                ),
                Content::MediaRequest { .. } => "media request".into(),
                Content::Other { content_type, body } => {
                    format!("{} bytes of type 0x{content_type:02x}", body.len())
                }
            };
            format!(
                "from {}: {body} (SNR {} dB, RSSI {rssi} dBm)",
                name_of(core, from),
                f32::from(*snr4) / 4.0
            )
        }
        Event::Progress { sent, total, .. } => {
            format!("fragment {sent} sent ({total} in the datagram)")
        }
        Event::GroupJoined { gid, name, admin } => format!(
            "added to group \"{name}\" ({}) by {}",
            hex::encode(&gid[..4]),
            name_of(core, admin)
        ),
        Event::GroupUpdated { gid } => format!("group {} updated", hex::encode(&gid[..4])),
        Event::GroupLeft { gid } => format!("removed from group {}", hex::encode(&gid[..4])),
        Event::GroupMessage {
            gid, from, content, ..
        } => {
            let body = match content {
                Content::Text { text, .. } => format!("\"{text}\""),
                Content::Location(p) => describe_position(p),
                Content::Voice { frames, data, .. } => format!(
                    "voice message, {:.1} s, {} bytes",
                    f32::from(*frames) * 0.04,
                    data.len()
                ),
                Content::Image {
                    full,
                    width,
                    height,
                    data,
                    ..
                } => format!(
                    "image {width}x{height}, {} ({} bytes)",
                    if *full { "full" } else { "preview" },
                    data.len()
                ),
                Content::MediaRequest { .. } => "media request".into(),
                Content::Other { content_type, body } => {
                    format!("{} bytes of type 0x{content_type:02x}", body.len())
                }
            };
            format!(
                "[group {}] {}: {body}",
                hex::encode(&gid[..4]),
                name_of(core, from)
            )
        }
        Event::Location {
            from,
            group,
            position,
            at,
            ..
        } => format!(
            "{}{} at {at}: {}",
            group.map_or(String::new(), |g| format!(
                "[group {}] ",
                hex::encode(&g[..4])
            )),
            name_of(core, from),
            describe_position(position)
        ),
        Event::SharesChanged => "a location share ended".into(),
        Event::IdentityConflict { node, identity } => format!(
            "{} (verified) now presents identity {}: NOT adopted, verify before accepting",
            name_of(core, node),
            lc_core::identity::fingerprint(identity).join(" ")
        ),
        Event::MediaRequested { from, full, .. } => format!(
            "{} asks for the {} image",
            name_of(core, from),
            if *full { "full" } else { "preview" }
        ),
        Event::Delivered { id } => format!("message {id} delivered"),
        Event::Failed { id, reason } => format!("message {id} failed: {reason:?}"),
    }
}

fn describe_position(p: &lc_proto::location::Position) -> String {
    let (lat, lon) = lc_core::location::to_degrees(p.lat, p.lon);
    let flags = match (p.sos(), p.end()) {
        (true, _) => " SOS",
        (_, true) => " (sharing ended)",
        _ => "",
    };
    format!(
        "position {lat:.5}, {lon:.5} ±{:.0} m{flags}",
        lc_core::location::radius_m(p.prec)
    )
}

fn find_contact(core: &Core, to: &str) -> Result<PubKey, String> {
    let wanted = to.to_lowercase();
    let mut hits: Vec<&lc_core::Contact> = core
        .contacts()
        .filter(|c| c.name.to_lowercase() == wanted || hex::encode(c.key).starts_with(&wanted))
        .collect();
    hits.sort_by_key(|c| c.key);
    match hits.as_slice() {
        [c] => Ok(c.key),
        [] => Err(format!("no contact matches \"{to}\"")),
        _ => Err(format!(
            "several contacts match \"{to}\": use a longer key prefix"
        )),
    }
}

/// Connects and waits for the node and the contact list.
fn connect(port: &str, verbose: bool) -> Result<Device, String> {
    let mut dev = Device::open(port)?;
    dev.verbose = verbose;
    if let Some(id) = dev.core.key() {
        println!("identity {}", lc_core::identity::fingerprint(id).join(" "));
    }
    let (mut node, mut contacts) = (false, false);
    let ok = dev.run(Duration::from_secs(20), |core, e, _| {
        println!("{}", describe(core, &e));
        match e {
            Event::Connected { .. } => node = true,
            Event::ContactsUpdated => contacts = true,
            _ => {}
        }
        if node && contacts {
            Flow::Stop
        } else {
            Flow::Continue
        }
    })?;
    if !ok || !node {
        return Err("the companion did not answer with its node and contacts".into());
    }
    Ok(dev)
}

fn info(port: &str, verbose: bool) -> Result<(), String> {
    let mut dev = connect(port, verbose)?;
    let now = dev.now();
    let (used, limit) = dev.core.budget(now);
    println!(
        "airtime budget: {} s used of {} s per hour",
        used / 1000,
        limit / 1000
    );
    for c in dev.core.contacts() {
        let path = c
            .out_path
            .as_ref()
            .map_or_else(|| "no path".to_owned(), |p| format!("{} hops", p.hops()));
        println!("  {}  {:<32} {path}", short(&c.key), c.name);
    }
    Ok(())
}

fn send(
    port: &str,
    to: &str,
    text: Option<String>,
    file: Option<PathBuf>,
    timeout_min: u64,
    verbose: bool,
) -> Result<(), String> {
    let mut dev = connect(port, verbose)?;
    let key = find_contact(&dev.core, to)?;
    let now = dev.now();
    let id = match (text, file) {
        (Some(t), _) => dev.core.send_text(&key, &t, unix_now(), now),
        (None, Some(f)) => {
            let body = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            let mut datagram = vec![lc_proto::content::FILE];
            datagram.extend_from_slice(&body);
            dev.core.send_datagram(&key, datagram, now)
        }
        (None, None) => return Err("give --text or --file".into()),
    }
    .map_err(|e| format!("{e:?}"))?;
    let mut result = None;
    let started = std::time::Instant::now();
    dev.run(Duration::from_secs(timeout_min * 60), |core, e, _| {
        println!(
            "[{:>5} s] {}",
            started.elapsed().as_secs(),
            describe(core, &e)
        );
        match e {
            Event::Delivered { id: i } if i == id => {
                result = Some(true);
                Flow::Stop
            }
            Event::Failed { id: i, .. } if i == id => {
                result = Some(false);
                Flow::Stop
            }
            _ => Flow::Continue,
        }
    })?;
    let now = dev.now();
    let (used, limit) = dev.core.budget(now);
    println!(
        "airtime budget: {} s used of {} s per hour",
        used / 1000,
        limit / 1000
    );
    match result {
        Some(true) => Ok(()),
        Some(false) => Err("not delivered".into()),
        None => Err("timed out".into()),
    }
}

fn listen(port: &str, minutes: u64, verbose: bool) -> Result<(), String> {
    let mut dev = connect(port, verbose)?;
    println!("listening for {minutes} min (Ctrl-C to stop)");
    dev.run(Duration::from_secs(minutes * 60), |core, e, _| {
        if !matches!(e, Event::Progress { .. }) {
            println!("{}", describe(core, &e));
        }
        Flow::Continue
    })?;
    Ok(())
}

fn sim(repeaters: usize, loss: u32, bytes: usize, seed: u64, cr: u8) -> Result<(), String> {
    let radio = LoRaParams::meshcore(8, 62_500, cr).ok_or("coding rate must be 5..8")?;
    if !(1..=10_048).contains(&bytes) {
        return Err("--bytes must be 1..10048".into());
    }
    let mut net = Net::new(radio, loss, seed);
    let a = net.add_companion(1, "alice");
    let mut chain = vec![a];
    for r in 0..repeaters {
        chain.push(net.add_repeater(10 + r as u8, &format!("repeater-{r}")));
    }
    let b = net.add_companion(2, "bob");
    chain.push(b);
    net.chain(&chain);
    net.know(a, b, true);
    net.know(b, a, true);
    net.connect(a);
    net.connect(b);
    net.run_until(10_000, |_| false);
    let key_b = net.key(b);
    let mut datagram = vec![lc_proto::content::FILE];
    datagram.extend((1..bytes).map(|i| i as u8));
    let id = net
        .with_core(a, |c, now| c.send_datagram(&key_b, datagram, now))
        .ok_or("no app")?
        .map_err(|e| format!("{e:?}"))?;
    println!(
        "alice -> {repeaters} repeater(s) -> bob, {bytes} bytes, {loss} % loss per link, SF8 / 62.5 kHz / CR 4/{cr}"
    );
    let t0 = net.now;
    let mut done = None;
    net.run_until(t0 + 180 * 60_000, |n| {
        for (who, i) in [("alice", a), ("bob", b)] {
            for e in n.take_events(i) {
                let t = (n.now - t0) as f64 / 1000.0;
                match &e {
                    Event::Progress { sent, total, .. } => {
                        if *sent == 1 || *sent % 16 == 0 {
                            println!("[{t:>7.1} s] {who}: {sent} fragment(s) sent ({total} in the datagram)");
                        }
                    }
                    Event::Received { content, .. } => {
                        println!("[{t:>7.1} s] {who}: received {} bytes", content.encode().len());
                    }
                    Event::Delivered { id: i } if *i == id => {
                        println!("[{t:>7.1} s] {who}: delivered");
                        done = Some(true);
                    }
                    Event::Failed { id: i, reason } if *i == id => {
                        println!("[{t:>7.1} s] {who}: failed ({reason:?})");
                        done = Some(false);
                    }
                    Event::SessionEstablished { .. } => println!("[{t:>7.1} s] {who}: secure session"),
                    _ => {}
                }
            }
        }
        done.is_some()
    });
    println!("network: {:?}", net.stats);
    for n in &net.nodes {
        println!(
            "  {:<12} {:>4} frames, {:>5.1} s on air",
            n.name,
            n.tx_frames,
            n.tx_airtime_ms as f64 / 1000.0
        );
    }
    match done {
        Some(true) => Ok(()),
        _ => Err("not delivered".into()),
    }
}

fn main() {
    let cli = Cli::parse();
    let r = match cli.command {
        Cmd::Info { port, verbose } => info(&port, verbose),
        Cmd::Send {
            port,
            to,
            text,
            file,
            timeout_min,
            verbose,
        } => send(&port, &to, text, file, timeout_min, verbose),
        Cmd::Listen {
            port,
            minutes,
            verbose,
        } => listen(&port, minutes, verbose),
        Cmd::Sim {
            repeaters,
            loss,
            bytes,
            seed,
            cr,
        } => sim(repeaters, loss, bytes, seed, cr),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
