//! Load test: N clients in one sector, each moving its own player at 20 Hz in
//! a 200 m square. Prints what was measured (single machine; not a network test).
//!
//!   KIT_RT_URL=ws://… KIT_RT_ENV=env_… KIT_RT_TOKENS=tokens.txt KIT_RT_SECONDS=20 \
//!   cargo run --release -p kit_realtime --example crowd

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec3;
use kit_realtime::event::Event;
use kit_realtime::protocol::NetTransform;
use kit_realtime::{Channel, Config, Realtime};

fn main() {
    let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let tokens: Vec<String> = std::fs::read_to_string(env("KIT_RT_TOKENS")).unwrap().lines().map(str::to_owned).collect();
    let secs: f64 = std::env::var("KIT_RT_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let cfg = Config { connect_url: env("KIT_RT_URL"), environment: env("KIT_RT_ENV"), world: "load".into(), party: None };
    let n = tokens.len();
    let mut clients: Vec<Realtime> = tokens
        .into_iter()
        .map(|t| {
            let mut rt = Realtime::connect(cfg.clone(), Arc::new(move || Ok(t.clone())));
            rt.enter_scene("scenes/plaza.kscene");
            rt
        })
        .collect();
    let start = Instant::now();
    let mut ready = vec![false; n];
    let mut scopes = std::collections::BTreeMap::<String, usize>::new();
    while ready.iter().any(|r| !r) {
        assert!(start.elapsed() < Duration::from_secs(120), "only {} of {n} joined", ready.iter().filter(|r| **r).count());
        for (i, rt) in clients.iter_mut().enumerate() {
            rt.flush();
            for (ch, e) in rt.poll() {
                if let (Channel::Sector, Event::Welcome { scope, .. }) = (&ch, &e) { *scopes.entry(scope.clone()).or_default() += 1; }
                if ch == Channel::Sector && e == Event::Ready { ready[i] = true; }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("{n} clients joined in {:.2?}: {scopes:?}", start.elapsed());

    // Everyone spawns a player at a pseudo-random spot.
    let spot = |i: usize, t: f32| {
        let a = i as f32 * 2.399 + t * 0.5;
        Vec3::new((i as f32 * 37.0 % 200.0) - 100.0 + a.cos() * 3.0, 0.0, (i as f32 * 61.0 % 200.0) - 100.0 + a.sin() * 3.0)
    };
    let mut ids = vec![0u32; n];
    for (i, rt) in clients.iter_mut().enumerate() {
        rt.spawn(Channel::Sector, "player", 0, &NetTransform { position: Some(spot(i, 0.0)), ..Default::default() });
        rt.interest(spot(i, 0.0), 32.0);
        rt.flush();
    }
    let mut received = vec![0u64; n];
    let mut errors = 0u64;
    let run = Instant::now();
    let mut next_send = 0.0;
    let mut sends = 0u64;
    while run.elapsed().as_secs_f64() < secs {
        let t = run.elapsed().as_secs_f64();
        let send = t >= next_send;
        if send { next_send += 0.05; }
        for (i, rt) in clients.iter_mut().enumerate() {
            if send && ids[i] != 0 {
                let p = spot(i, t as f32);
                rt.transform(Channel::Sector, ids[i], &NetTransform { position: Some(p), ..Default::default() });
                rt.interest(p, 32.0);
                sends += 1;
            }
            rt.flush();
            for (_, e) in rt.poll() {
                match e {
                    Event::Spawned { id, .. } => ids[i] = id,
                    Event::Transform { .. } | Event::Entity { .. } => received[i] += 1,
                    Event::Error { .. } => errors += 1,
                    _ => {}
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let el = run.elapsed().as_secs_f64();
    received.sort();
    let total: u64 = received.iter().sum();
    println!(
        "{el:.1}s: sent {sends} updates ({:.0}/s across all clients); each client received median {:.0}/s, p90 {:.0}/s, max {:.0}/s entity updates; {} errors; total delivered {:.0}/s",
        sends as f64 / el,
        received[n / 2] as f64 / el,
        received[n * 9 / 10] as f64 / el,
        received[n - 1] as f64 / el,
        errors,
        total as f64 / el
    );
}
