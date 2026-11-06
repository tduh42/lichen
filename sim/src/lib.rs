//! Runs lichen apps (`lc_core::Core`) on oxymesh's simulated MeshCore devices.

use lc_core::{Config, Core, Event};
use mc_core::Instant;
use mc_sim::World;

/// A lichen app attached to device `node` of the world.
pub struct App {
    pub core: Core,
    pub node: usize,
    pub events: Vec<Event>,
}

impl App {
    pub fn new(node: usize, seed: u8) -> Self {
        Self {
            core: Core::new(Config::new("lichen-sim", [seed ^ 0x5A; 32], [seed; 32])),
            node,
            events: Vec::new(),
        }
    }
}

/// Apps `i` and `j` learn each other's identity, verified, as if they had scanned each
/// other's contact card (ADR-0013). Needs both contact lists loaded.
pub fn introduce(apps: &mut [App], i: usize, j: usize) {
    let id = |a: &App| a.core.key().copied();
    let node = |a: &App| a.core.node_key().copied();
    let (Some(ii), Some(ij), Some(ni), Some(nj)) =
        (id(&apps[i]), id(&apps[j]), node(&apps[i]), node(&apps[j]))
    else {
        return;
    };
    apps[i].core.set_identity(&nj, ij, true);
    apps[j].core.set_identity(&ni, ii, true);
}

pub fn ms(t: Instant) -> u64 {
    t.as_nanos() / 1_000_000
}

/// Exchanges frames between the apps and their devices.
pub fn pump(world: &mut World, apps: &mut [App]) {
    let now = ms(world.now);
    for app in apps.iter_mut() {
        app.core.tick(now);
        while let Some(f) = app.core.poll_companion(now) {
            for r in world.app(app.node, &f) {
                app.core.on_companion_frame(&r, now);
            }
        }
        let pushed = std::mem::take(&mut world.nodes[app.node].app);
        for r in pushed {
            app.core.on_companion_frame(&r, now);
        }
        while let Some(e) = app.core.poll_event() {
            app.events.push(e);
        }
    }
}

/// Steps the world in 5 ms steps, pumping the apps, until `done` or `max_ms`.
pub fn run(
    world: &mut World,
    apps: &mut [App],
    max_ms: u64,
    mut done: impl FnMut(&[App]) -> bool,
) -> bool {
    let end = ms(world.now) + max_ms;
    while ms(world.now) < end {
        pump(world, apps);
        if done(apps) {
            return true;
        }
        world.step(mc_core::Duration::from_millis(5));
    }
    false
}
