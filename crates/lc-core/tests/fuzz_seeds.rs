//! Seeds for the `snapshot_restore` fuzz target (review of 2026-11-15, SR-37): real group and
//! invitation snapshots from the fake network, written to `fuzz/corpus/snapshot_restore`.
//! `scripts/fuzz.sh` runs it before that target.

use std::path::PathBuf;

use lc_core::fakenet::Net;
use lc_meshcore::airtime::LoRaParams;

const MIN: u64 = 60_000;

#[test]
#[ignore]
fn snapshot_seeds() {
    let mut net = Net::new(LoRaParams::meshcore(8, 62_500, 5).unwrap(), 0, 7);
    let nodes: Vec<usize> = ["alice", "bob", "carol"]
        .iter()
        .enumerate()
        .map(|(i, n)| net.add_companion(1 + i as u8, n))
        .collect();
    let hub = net.add_repeater(50, "hub");
    for &m in &nodes {
        net.link(m, hub);
        net.connect(m);
    }
    assert!(net.run_until(net.now + 20_000, |n| {
        nodes
            .iter()
            .all(|&m| n.nodes[m].core.as_ref().is_some_and(|c| c.ready()))
    }));
    for &m in &nodes[1..] {
        net.know(nodes[0], m, true);
        net.know(m, nodes[0], true);
        net.introduce(nodes[0], m);
    }
    let keys: Vec<[u8; 32]> = nodes[1..].iter().map(|&i| net.key(i)).collect();
    let gid = net
        .with_core(nodes[0], |c, now| {
            c.create_group("Rando Vercors", &keys, now)
        })
        .unwrap()
        .unwrap();
    net.run_until(net.now + 30 * MIN, |_| false);
    // Carol, who never verified alice in person, asks before joining a second group: an
    // invitation, with the keys that came for it.
    let (alice_node, alice_id) = (
        net.key(nodes[0]),
        net.with_core_ref(nodes[0], |c| *c.key().unwrap()).unwrap(),
    );
    net.with_core(nodes[2], |c, _| {
        c.set_group_consent(true);
        c.set_identity(&alice_node, alice_id, false);
    });
    let second = net
        .with_core(nodes[0], |c, now| {
            c.create_group("Refuge tonight", &keys, now)
        })
        .unwrap()
        .unwrap();
    net.run_until(net.now + 20 * MIN, |_| false);

    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/snapshot_restore");
    std::fs::create_dir_all(&dir).unwrap();
    let mut written = 0;
    for (i, &m) in nodes.iter().enumerate() {
        for (g, name) in [(gid, "group"), (second, "group2")] {
            if let Some(s) = net.with_core_ref(m, |c| c.group_snapshot(&g)).flatten() {
                std::fs::write(dir.join(format!("{name}-{i}")), s).unwrap();
                written += 1;
            }
        }
    }
    let invitation = net
        .with_core_ref(nodes[2], |c| c.invitation_snapshot(&second))
        .flatten()
        .expect("carol holds an invitation");
    std::fs::write(dir.join("invitation"), invitation).unwrap();
    println!("{} seeds in {}", written + 1, dir.display());
}

/// Seeds for the `group_open` fuzz target (SR-37): well-formed group envelopes of every shape
/// (fragment index and total, copy, padded or not, short and full bodies), sealed under a key
/// the target does not hold, so it starts past the parser and checks that none is accepted.
#[test]
#[ignore]
fn group_open_seeds() {
    use lc_proto::group::{GroupHeader, PAYLOAD_MAX, seal};
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/group_open");
    std::fs::create_dir_all(&dir).unwrap();
    let key = [42u8; 32];
    let mut written = 0;
    for (total, index) in [(1u8, 0u8), (2, 0), (2, 1), (3, 2)] {
        for copy in 0..2u8 {
            for len in [0usize, 1, 7, 59, 100, PAYLOAD_MAX] {
                let h = GroupHeader {
                    gid_hash: 0xBEEF,
                    sender: 3 + index,
                    key_epoch: copy,
                    counter: 1000 + u32::from(index),
                    index,
                    total,
                    copy,
                };
                let plain: Vec<u8> = (0..len).map(|i| i as u8).collect();
                let mut out = [0u8; 256];
                if let Ok(n) = seal(&key, &h, &plain, &mut out) {
                    std::fs::write(
                        dir.join(format!("env-{total}-{index}-{copy}-{len}")),
                        &out[..n],
                    )
                    .unwrap();
                    written += 1;
                }
            }
        }
    }
    assert!(written > 20, "{written}");
    println!("{written} seeds in {}", dir.display());
}
