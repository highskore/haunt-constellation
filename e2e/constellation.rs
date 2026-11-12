//! End to end against a real haunt core. Copy into haunt's `cli/tests/` and run
//! `cargo test -p haunt-cli --features plugins --test constellation` (see README).
#![cfg(feature = "plugins")]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haunt_core::plugin_panel::{PanelNodeKind, PanelPlacement, PanelShape, PanelTone};
use haunt_core::plugin_settings::PluginConfig;
use haunt_core::plugins::HostRequest;
use haunt_core::plugin_store::{Store, attach};
use haunt_core::plugins::PluginHost;
use haunt_core::{Core, SurfaceEnv};

const ID: &str = "dev.haunt.constellation";

fn repo() -> PathBuf {
    std::env::var_os("HAUNT_PLUGINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../haunt-plugins"))
        .join("haunt-constellation")
}

fn until(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send(sock: &mut UnixStream, env: &SurfaceEnv, nonce: bool, cwd: &str, rest: &str) {
    let nonce = if nonce { format!(r#","nonce":"{}""#, env.nonce) } else { String::new() };
    let line = format!(r#"{{"v":1,"agent":"claude","surface_id":"{}"{nonce},"session_id":"s1","cwd":"{cwd}",{rest}}}"#, env.surface_id);
    sock.write_all(format!("{line}\n").as_bytes()).unwrap();
}

/// The last show's generation and shapes.
fn last_show(set: &Mutex<haunt_core::plugin_store::PluginSet>) -> Option<(u64, Vec<PanelShape>)> {
    set.lock().unwrap().take_requests().into_iter().rev().find_map(|r| match r {
        HostRequest::ShowPanel { plugin_id, panel, .. } if plugin_id == ID => {
            assert_eq!((panel.id.as_str(), panel.placement, panel.title.as_str()), ("sky", PanelPlacement::Window, "Constellation"));
            match &panel.nodes[0].kind {
                PanelNodeKind::Canvas { shapes, .. } => Some((panel.generation, shapes.clone())),
                k => panic!("{k:?}"),
            }
        }
        _ => None,
    })
}

fn last_sky(set: &Mutex<haunt_core::plugin_store::PluginSet>) -> Option<Vec<PanelShape>> {
    last_show(set).map(|(_, s)| s)
}

fn labels(shapes: &[PanelShape]) -> Vec<(String, PanelTone)> {
    shapes.iter().filter_map(|s| if let PanelShape::Label { text, tone, .. } = s { Some((text.clone(), *tone)) } else { None }).collect()
}

#[test]
fn the_window_shows_one_constellation_per_repo() {
    let dir = repo();
    if !dir.join("haunt-plugin.toml").is_file() {
        eprintln!("skipped: haunt-constellation not found; set HAUNT_PLUGINS_DIR");
        return;
    }
    let home = PathBuf::from(format!("/tmp/haunt-fp-sky-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .unwrap();
    assert!(status.success());
    let o = Command::new(env!("CARGO_BIN_EXE_haunt")).args(["plugin", "install", dir.to_str().unwrap(), "--yes"]).env("HAUNT_HOME", &home).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));

    let core = Core::new(&home);
    core.start().unwrap();
    let (api, web, forged) = (core.register_surface("/tmp").unwrap(), core.register_surface("/tmp").unwrap(), core.register_surface("/tmp").unwrap());
    let set = Store::open(&home).unwrap().load_all(&PluginHost::new().unwrap(), Some(&core.handle())).unwrap();
    assert!(set.errors().is_empty(), "{:?}", set.errors());
    let set = Arc::new(Mutex::new(set));
    attach(&core, &set);

    let mut sock = UnixStream::connect(&api.socket_path).unwrap();
    send(&mut sock, &web, true, "/tmp/web", r#""event":"prompt_submit","query":"go""#);
    send(&mut sock, &web, true, "/tmp/web", r#""event":"subagent_start","agent_id":"a1","agent_type":"Explore""#);
    send(&mut sock, &forged, false, "/tmp/evil", r#""event":"prompt_submit","query":"go""#);
    send(&mut sock, &api, true, "/tmp/api", r#""event":"permission_seen","tool_name":"Bash","tool_input":{"command":"git push"}"#);
    until("the api tab to need you", || core.snapshot().groups.first().is_some_and(|g| g.id == "needs-you"));
    std::thread::sleep(Duration::from_millis(200));

    set.lock().unwrap().invoke(ID, "show", true).unwrap();
    let sky = last_sky(&set).expect("the window opens");
    assert_eq!(labels(&sky), [("needs you".to_owned(), PanelTone::Warn), ("web · 1 subagent".to_owned(), PanelTone::Dim)], "the forged repo isn't in the sky");
    assert!(sky.iter().any(|s| matches!(s, PanelShape::Dot { warn: true, pulse: true, .. })));

    for i in 0..3 {
        send(&mut sock, &web, true, "/tmp/web", &format!(r#""event":"tool_start","tool_name":"Read","tool_use_id":"t{i}""#));
    }
    send(&mut sock, &web, true, "/tmp/web", r#""event":"subagent_start","agent_id":"a2","agent_type":"Explore""#);
    std::thread::sleep(Duration::from_millis(300));
    let sky = last_sky(&set).expect("the sky redraws");
    assert_eq!(labels(&sky)[0].0, "web · 2 subagents", "the busier repo moves to the middle");

    set.lock().unwrap().set_config(&PluginConfig::parse(&format!("[plugins.\"{ID}\"]\nlabels = false\nbackground = false\n")));
    let (generation, sky) = last_show(&set).expect("an open window follows its settings");
    assert!(labels(&sky).is_empty());
    assert_eq!(sky.iter().filter(|s| matches!(s, PanelShape::Dot { .. })).count(), 4, "two stars and two moons");

    set.lock().unwrap().panel_action(ID, "sky", generation, "").unwrap();
    send(&mut sock, &web, true, "/tmp/web", r#""event":"stop""#);
    std::thread::sleep(Duration::from_millis(300));
    assert!(last_sky(&set).is_none(), "a closed window stays closed");

    drop(core);
    let _ = std::fs::remove_dir_all(&home);
}
