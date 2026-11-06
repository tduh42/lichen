//! Prints when each message reaches the app in the demo, as the iOS app runs it with
//! `-media YES` (diagnostic, ignored by default).
use lc_ffi::{DemoNetwork, LichenCore, LichenEvent, Message};

#[test]
#[ignore]
fn timeline() {
    let demo = DemoNetwork::new();
    let core = LichenCore::new("t".into(), vec![9; 32], None, None).unwrap();
    core.set_clock(1_793_000_000_000, 0);
    core.connect(0);
    let mut voice_at: Option<u64> = None;
    let mut now = 0;
    while now < 6 * 60_000 {
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
                LichenEvent::ContactsUpdated if voice_at.is_none() => voice_at = Some(now + 20_000),
                LichenEvent::MessageChanged { message } => {
                    let what = match &message.message {
                        Message::Text { text, .. } => format!("text {text:?}"),
                        Message::Voice { frames, .. } => format!("voice {frames} frames"),
                        Message::Image { full, data, .. } => {
                            format!("image full={full} {} B", data.len())
                        }
                        Message::Other { content_type, .. } => format!("other {content_type}"),
                    };
                    println!(
                        "{t:>6.1} s  #{} out={} {:?} {what}",
                        message.id, message.outgoing, message.status
                    );
                }
                LichenEvent::Failed { id, reason } => {
                    println!("{t:>6.1} s  failed {id} {reason:?}")
                }
                _ => {}
            }
        }
        if voice_at.is_some_and(|t| now >= t)
            && let Some(c) = core.contacts().into_iter().find(|c| c.name == "Camille")
        {
            voice_at = Some(u64::MAX);
            let pcm: Vec<i16> = (0..32_000).map(|i| ((i % 53) as i16 - 26) * 200).collect();
            let r = core.send_voice(c.key, lc_ffi::prepare_voice(pcm), 1, now);
            println!(
                "{:>6.1} s  send_voice -> {:?}",
                now as f64 / 1000.0,
                r.map(|m| m.id)
            );
        }
        now += 200;
    }
}
