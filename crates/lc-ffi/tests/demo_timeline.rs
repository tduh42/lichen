//! Prints when each message reaches the app in the demo, as the iOS app runs it with
//! `-media YES` (diagnostic, ignored by default).
use lc_ffi::{DemoNetwork, LichenCore, LichenEvent, Message};

#[test]
#[ignore]
fn timeline() {
    let demo = DemoNetwork::new();
    let core = LichenCore::new("t".into(), vec![7; 32], vec![107; 32], None).unwrap();
    core.set_clock(1_793_000_000_000, 0);
    core.connect(0);
    let mut voice_at: Option<u64> = None;
    let mut now = 0;
    while now < 20 * 60_000 {
        for f in demo.read(now) {
            core.on_companion_frame(f, now);
        }
        core.set_clock(1_793_000_000_000 + now, now);
        core.tick(now);
        while let Some(f) = core.poll_companion(now) {
            demo.write(f, now);
        }
        while let Some(e) = core.poll_event() {
            let t = now as f64 / 1000.0;
            match e {
                LichenEvent::ContactsUpdated if voice_at.is_none() => voice_at = Some(40_100),
                LichenEvent::MessageChanged { message } => {
                    let what = match &message.message {
                        Message::Text { text, .. } => format!("text {text:?}"),
                        Message::Voice { frames, .. } => format!("voice {frames} frames"),
                        Message::Image { full, data, .. } => {
                            format!("image full={full} {} B", data.len())
                        }
                        Message::Announce { name, size, .. } => {
                            format!("announce {name:?} {size} B")
                        }
                        Message::Other { content_type, .. } => format!("other {content_type}"),
                    };
                    println!(
                        "{t:>6.1} s  #{} out={} {:?} {what}",
                        message.id, message.outgoing, message.status
                    );
                    if matches!(message.message, Message::Image { full: false, .. })
                        && !message.outgoing
                    {
                        let r = core.request_full_image(message.id, now);
                        println!("{t:>6.1} s  request full -> {r:?}");
                    }
                }
                LichenEvent::Failed { id, reason } => {
                    println!("{t:>6.1} s  failed {id} {reason:?}");
                }
                LichenEvent::IdentityLearned { node, changed, .. } => {
                    println!(
                        "{t:>6.1} s  identity of {:02x} learned (changed {changed})",
                        node[0]
                    );
                }
                LichenEvent::GroupJoined { name, .. } => println!("{t:>6.1} s  joined {name}"),
                LichenEvent::SessionEstablished { peer } => {
                    println!("{t:>6.1} s  session with {:02x}", peer[0]);
                }
                LichenEvent::MediaRequested { .. } => println!("{t:>6.1} s  media requested"),
                LichenEvent::GroupUpdated { .. } => {
                    let g = core.groups();
                    println!(
                        "{t:>6.1} s  group updated, keys {:?}",
                        g.iter().map(|g| g.keys_known).collect::<Vec<_>>()
                    );
                }
                _ => {}
            }
        }
        if voice_at.is_some_and(|t| now >= t)
            && let Some(c) = core.contacts().into_iter().find(|c| c.name == "Camille")
        {
            voice_at = Some(u64::MAX);
            let r = core.send_text(c.key, "On my way".into(), 1, now);
            println!(
                "{:>6.1} s  send_text -> {:?}",
                now as f64 / 1000.0,
                r.map(|m| m.id)
            );
        }
        now += 100;
    }
}
