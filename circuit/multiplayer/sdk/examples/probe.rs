//! Two native clients in one sector: A spawns and moves a player, B watches.
//! Prints measured numbers only.
//!
//!   KIT_RT_URL=ws://127.0.0.1:8787/api/v1/realtime/connect KIT_RT_ENV=env_… \
//!   KIT_RT_TOKEN_A=v1… KIT_RT_TOKEN_B=v1… cargo run -p kit_realtime --example probe

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use kit_realtime::event::Event;
use kit_realtime::protocol::{NetTransform, Value};
use kit_realtime::{Channel, Config, Realtime};

fn var(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("set {k}"))
}

fn main() {
    let cfg =
        Config { connect_url: var("KIT_RT_URL"), environment: var("KIT_RT_ENV"), world: "main".into(), party: None };
    let (ta, tb) = (var("KIT_RT_TOKEN_A"), var("KIT_RT_TOKEN_B"));
    let mut a = Realtime::connect(cfg.clone(), Arc::new(move || Ok(ta.clone())));
    let mut b = Realtime::connect(cfg, Arc::new(move || Ok(tb.clone())));
    a.enter_scene("scenes/level2.kscene");
    b.enter_scene("scenes/level2.kscene");

    let start = Instant::now();
    let wait = |rt: &mut Realtime, what: &str, f: &mut dyn FnMut(&Channel, &Event) -> bool| {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            rt.flush();
            for (ch, e) in rt.poll() {
                if let Event::Error { message, .. } = &e {
                    eprintln!("error: {message}");
                }
                if let Event::Status(s) = &e {
                    eprintln!("status: {s:?}");
                }
                if f(&ch, &e) {
                    return;
                }
            }
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let mut scope = String::new();
    wait(&mut a, "A ready", &mut |ch, e| {
        if let Event::Welcome { scope: s, .. } = e {
            if *ch == Channel::Sector {
                scope = s.clone();
            }
        }
        *ch == Channel::Sector && *e == Event::Ready
    });
    wait(&mut b, "B ready", &mut |ch, e| *ch == Channel::Sector && *e == Event::Ready);
    println!("both in {scope} after {:?}", start.elapsed());

    let req = a.spawn(
        Channel::Sector,
        "player",
        0,
        &NetTransform { position: Some(Vec3::ZERO), rotation: Some(Quat::IDENTITY), ..Default::default() },
    );
    let mut id = 0;
    wait(
        &mut a,
        "spawned",
        &mut |_, e| matches!(e, Event::Spawned { request, id: i } if *request == req && { id = *i; true }),
    );
    wait(&mut b, "entity", &mut |_, e| matches!(e, Event::Entity { id: i, .. } if *i == id));

    // Move: A sends a position, B sees it. Round trip measured on one machine.
    let mut samples = Vec::new();
    for n in 1..=50 {
        let x = n as f32;
        let sent = Instant::now();
        a.transform(
            Channel::Sector,
            id,
            &NetTransform { position: Some(Vec3::new(x, 0.0, 0.0)), ..Default::default() },
        );
        a.flush();
        wait(
            &mut b,
            "transform",
            &mut |_, e| matches!(e, Event::Transform { id: i, transform } if *i == id && transform.position.map(|p| p.x) == Some(x)),
        );
        samples.push(sent.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "back-to-back moves (faster than the 20 Hz cap): median {:.1} ms, p90 {:.1} ms between deliveries",
        samples[25], samples[45]
    );
    // Latency: moves spaced like a 15 Hz game, so the rate cap doesn't apply.
    let mut lat = Vec::new();
    for n in 51..=100 {
        std::thread::sleep(Duration::from_millis(66));
        let x = n as f32;
        let sent = Instant::now();
        a.transform(
            Channel::Sector,
            id,
            &NetTransform { position: Some(Vec3::new(x, 0.0, 0.0)), ..Default::default() },
        );
        a.flush();
        wait(
            &mut b,
            "transform",
            &mut |_, e| matches!(e, Event::Transform { id: i, transform } if *i == id && transform.position.map(|p| p.x) == Some(x)),
        );
        lat.push(sent.elapsed().as_secs_f64() * 1000.0);
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "A→scope→B latency over 50 spaced moves: median {:.2} ms, p90 {:.2} ms, max {:.2} ms (a local Realtime service on one machine; not a production network number)",
        lat[25], lat[45], lat[49]
    );

    a.set_state(Channel::World, "weather", &Value::Str("rain".into()), kit_realtime::protocol::ANY_REVISION);
    a.flush();
    wait(&mut b, "state", &mut |ch, e| {
        *ch == Channel::World
            && matches!(e, Event::State { key, value, .. } if key == "weather" && *value == Value::Str("rain".into()))
    });
    println!("world state reached B");
    a.modify(Channel::Sector, 0, 0, "tree:1", &Value::Str("removed".into()), 0);
    a.flush();
    b.watch_chunks(Channel::Sector, &[((0, 0), 0)]);
    wait(&mut b, "world mod", &mut |_, e| matches!(e, Event::WorldMod { key, .. } if key == "tree:1"));
    println!("world modification reached B; done in {:?}", start.elapsed());
}
