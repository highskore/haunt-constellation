//! Constellation: the fleet as a night sky in its own window. Each repo is a constellation and each
//! agent a star, brighter the busier it is; subagents are moons and a star that needs you pulses orange.
// wit-bindgen's flattened `notify` import has 14 params.
#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({ path: "wit/haunt-plugin.wit", world: "plugin" });

// Shared with the other fleet-following plugins; not every helper is used here.
#[allow(dead_code)]
mod fleet;
mod sky;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use fleet::{Ev, Fleet};
use haunt::plugin::log::log;
use haunt::plugin::panel::{self, CanvasNode, Dot, Label, Line, Node, PanelNode, Placement, Shape, Tone};
use haunt::plugin::types::LogLevel;
use haunt::plugin::{palette, timer};
use serde::Deserialize;
use sky::Constellation;

const PANEL: &str = "sky";
const W: f32 = 600.0;
const H: f32 = 300.0;
/// Busyness is tool calls over this window.
const RATE_MS: u64 = 5 * 60_000;
const TICK_SECS: u32 = 15;
/// How long a constellation takes to drift to its new place.
const EASE_MS: u32 = 2500;
/// A finished agent fades from the sky after this long; closed tabs send no event.
const IDLE_MS: u64 = 60 * 60_000;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Faint background stars, for the look only; they stand for nothing.
    pub background: bool,
    pub labels: bool,
    pub unverified: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { background: true, labels: true, unverified: false }
    }
}

#[derive(Default)]
struct Live {
    settings: Settings,
    fleet: Fleet,
    calls: BTreeMap<String, VecDeque<u64>>,
    open: bool,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    f(LIVE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Live::default))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Fixed faint dots, the same every time, scaled from the mockup's sky.
const BACKGROUND: [(f32, f32); 8] = [(12.0, 20.0), (140.0, 12.0), (210.0, 30.0), (280.0, 140.0), (160.0, 138.0), (20.0, 128.0), (250.0, 60.0), (100.0, 80.0)];

/// A shape's key, so Haunt eases it to its next place: at most 64 characters and unique per sky.
fn key(kind: &str, name: &str) -> Option<String> {
    Some(if name.chars().count() <= 48 { format!("{kind}:{name}") } else { format!("{kind}:{}~{:08x}", name.chars().take(39).collect::<String>(), sky::hash(name)) })
}

pub fn shapes(sky: &[Constellation], s: &Settings) -> Vec<Shape> {
    let (kx, ky) = (W / 300.0, H / 150.0);
    let mut out = Vec::new();
    if s.background {
        out.extend(BACKGROUND.iter().map(|(x, y)| Shape::Dot(Dot { x: x * kx, y: y * ky, r: 1.2, shade: 0.3, warn: false, pulse: false, key: None })));
    }
    for c in sky {
        out.extend(c.lines.iter().enumerate().map(|(i, ((x1, y1), (x2, y2)))| Shape::Line(Line { x1: *x1, y1: *y1, x2: *x2, y2: *y2, shade: 0.2, key: key(&format!("line{i}"), &c.repo) })));
    }
    for c in sky {
        for st in &c.stars {
            out.push(Shape::Dot(Dot { x: st.x, y: st.y, r: st.radius * 2.0, shade: st.brightness, warn: st.needs_you, pulse: st.needs_you, key: key("star", &st.surface) }));
            for m in 0..st.moons.min(8) {
                let a = m as f32 * std::f32::consts::TAU / st.moons.min(8) as f32;
                let d = st.radius * 2.0 + 14.0;
                out.push(Shape::Dot(Dot { x: st.x + d * a.cos(), y: st.y + d * a.sin(), r: 2.0, shade: 0.7, warn: false, pulse: false, key: key(&format!("moon{m}"), &st.surface) }));
            }
        }
        if s.labels {
            let needs = c.stars.iter().any(|st| st.needs_you);
            out.push(Shape::Label(Label { x: c.label_at.0, y: c.label_at.1, text: c.label.clone(), tone: if needs { Tone::Warn } else { Tone::Dim }, key: key("label", &c.repo) }));
        }
    }
    out
}

/// Drops agents that finished over an hour ago.
fn prune(l: &mut Live, now: u64) {
    let old: Vec<String> = l.fleet.agents.iter().filter(|(_, a)| matches!(a.state, fleet::State::Done | fleet::State::Failed) && now.saturating_sub(a.since) > IDLE_MS).map(|(s, _)| s.clone()).collect();
    for s in old {
        l.fleet.remove(&s);
        l.calls.remove(&s);
    }
}

fn show(l: &Live, now: u64) {
    let rate: BTreeMap<String, u32> = l.calls.iter().map(|(s, q)| (s.clone(), q.iter().filter(|t| now.saturating_sub(**t) < RATE_MS).count() as u32)).collect();
    let canvas = CanvasNode { width: W as u32, height: H as u32, shapes: shapes(&sky::sky(&l.fleet, &rate, W, H), &l.settings), ease_ms: Some(EASE_MS) };
    if let Err(e) = panel::show(PANEL, Placement::Window, "Constellation", &[PanelNode { parent: None, node: Node::Canvas(canvas) }]) {
        log(LogLevel::Warn, &format!("constellation: not shown: {e}"));
    }
}

struct Plugin;

impl Guest for Plugin {
    fn init(json: String) {
        with(|l| l.settings = serde_json::from_str(&json).unwrap_or_default());
        palette::command("show", "Constellation", None);
    }

    fn on_settings_changed(json: String) {
        palette::command("show", "Constellation", None);
        with(|l| {
            l.settings = serde_json::from_str(&json).unwrap_or_default();
            if l.open {
                show(l, now_ms());
            }
        });
    }

    fn on_event(event: AgentEvent) {
        let Some(surface) = event.surface.as_deref() else { return };
        with(|l| {
            if !event.verified && !l.settings.unverified {
                return;
            }
            let now = now_ms();
            let ev = Ev {
                surface,
                kind: &event.kind,
                cwd: event.cwd.as_deref(),
                summary: event.summary.as_deref(),
                subagent: event.agent_id.is_some(),
                agent_id: event.agent_id.as_deref(),
            };
            let changed = l.fleet.apply(&ev, now).is_some();
            if matches!(event.kind.as_str(), "tool_start" | "subagent_tool") {
                let q = l.calls.entry(surface.to_owned()).or_default();
                q.push_back(now);
                while q.front().is_some_and(|t| now.saturating_sub(*t) >= RATE_MS) || q.len() > 4096 {
                    q.pop_front();
                }
            }
            if l.open && (changed || event.kind.starts_with("subagent")) {
                show(l, now);
            }
        });
    }

    fn palette_items(_mode: String, _query: String) -> Vec<PaletteItem> {
        Vec::new()
    }

    fn on_action(action_id: String, payload: String) {
        with(|l| match (action_id.as_str(), payload.as_str()) {
            ("show", _) => {
                l.open = true;
                prune(l, now_ms());
                show(l, now_ms());
                let _ = timer::every(TICK_SECS);
            }
            ("", p) if p == format!("panel:{PANEL}") => {
                l.open = false;
                timer::stop();
            }
            _ => {}
        });
    }

    fn row_menu(_surface: String) -> Vec<MenuItem> {
        Vec::new()
    }

    fn on_row_moved(_surface: String, _group: String, _before: Option<String>) {}

    fn on_theme_changed() {}

    fn on_tick(now_ms: u64) {
        with(|l| {
            prune(l, now_ms);
            if l.open {
                show(l, now_ms);
            }
        });
    }

    fn on_exec(_job: u64, _outcome: Result<haunt::plugin::exec::Output, String>) {}

    fn on_away_changed(_away: bool, _away_ms: u64) {}

    fn on_hover_hold(_held: bool, _surface: String) {}

    fn on_fs_tree(_job: u64, _outcome: Result<haunt::plugin::fs_list::TreeResult, String>) {}
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet() -> Fleet {
        let mut f = Fleet::default();
        let ev = |surface, kind, cwd| Ev { surface, kind, cwd: Some(cwd), ..Ev::default() };
        f.apply(&ev("a", "permission_seen", "/w/api"), 0);
        f.apply(&ev("w", "prompt_submit", "/w/web"), 0);
        f.apply(&Ev { agent_id: Some("x"), ..ev("w", "subagent_start", "/w/web") }, 0);
        f.apply(&Ev { agent_id: Some("y"), ..ev("w", "subagent_start", "/w/web") }, 0);
        f.apply(&ev("i", "stop", "/w/infra"), 0);
        f
    }

    #[test]
    fn the_sky_has_the_mockups_parts() {
        let s = Settings::default();
        let out = shapes(&sky::sky(&fleet(), &BTreeMap::new(), W, H), &s);
        let dots: Vec<&Dot> = out.iter().filter_map(|x| if let Shape::Dot(d) = x { Some(d) } else { None }).collect();
        assert_eq!(dots.len(), 8 + 3 + 2, "background, a star per agent, a moon per running subagent");
        assert!(dots.iter().any(|d| d.warn && d.pulse), "the star that needs you pulses orange");
        let labels: Vec<(&str, bool)> = out.iter().filter_map(|x| if let Shape::Label(l) = x { Some((l.text.as_str(), matches!(l.tone, Tone::Warn))) } else { None }).collect();
        assert_eq!(labels, [("needs you", true), ("web · 2 subagents", false), ("infra · idle", false)], "live repos before idle ones");
        for d in &dots {
            assert!((0.0..=W).contains(&d.x) && (0.0..=H).contains(&d.y));
        }
    }

    #[test]
    fn moving_shapes_have_short_unique_keys_so_haunt_eases_them() {
        let out = shapes(&sky::sky(&fleet(), &BTreeMap::new(), W, H), &Settings::default());
        let keys: Vec<String> = out
            .iter()
            .filter_map(|x| match x {
                Shape::Dot(d) => d.key.clone(),
                Shape::Line(l) => l.key.clone(),
                Shape::Label(l) => l.key.clone(),
                Shape::Rect(r) => r.key.clone(),
            })
            .collect();
        assert_eq!(keys.len(), out.len() - 8, "all but the fixed background");
        assert_eq!(keys.iter().collect::<std::collections::BTreeSet<_>>().len(), keys.len());
        assert!(key("star", &"x".repeat(200)).unwrap().chars().count() <= 64);
    }

    #[test]
    fn finished_agents_fade_after_an_hour() {
        let mut l = Live { fleet: fleet(), ..Live::default() };
        prune(&mut l, IDLE_MS);
        assert_eq!(l.fleet.agents.len(), 3);
        prune(&mut l, IDLE_MS + 1);
        assert_eq!(l.fleet.agents.keys().collect::<Vec<_>>(), ["a", "w"], "infra finished at 0");
    }

    #[test]
    fn background_and_labels_can_go() {
        let out = shapes(&sky::sky(&fleet(), &BTreeMap::new(), W, H), &Settings { background: false, labels: false, ..Settings::default() });
        assert_eq!(out.iter().filter(|x| matches!(x, Shape::Dot(_))).count(), 5);
        assert!(!out.iter().any(|x| matches!(x, Shape::Label(_))));
    }

    #[test]
    fn manifest_asks_for_what_it_uses_and_defaults_match() {
        let m = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/haunt-plugin.toml")).unwrap();
        let t: toml::Table = toml::from_str(&m).unwrap();
        let caps = t["capabilities"].as_table().unwrap();
        assert_eq!(caps.keys().collect::<Vec<_>>(), ["events", "panel", "timer"]);
        let defaults: serde_json::Map<String, serde_json::Value> =
            t["settings"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), serde_json::to_value(&v["default"]).unwrap())).collect();
        assert_eq!(serde_json::from_value::<Settings>(serde_json::Value::Object(defaults)).unwrap(), Settings::default());
    }
}
