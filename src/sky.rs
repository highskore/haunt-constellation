//! The fleet as a night sky: one constellation per repo, one star per agent. Free of host calls.

use std::collections::BTreeMap;

use crate::fleet::{Fleet, State};

#[derive(Debug, Clone, PartialEq)]
pub struct Star {
    pub surface: String,
    pub x: f32,
    pub y: f32,
    /// 1 to 4: busier agents are bigger and brighter.
    pub radius: f32,
    /// 0.35 (idle) to 1.
    pub brightness: f32,
    pub needs_you: bool,
    pub moons: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constellation {
    pub repo: String,
    pub stars: Vec<Star>,
    /// Lines between consecutive stars.
    pub lines: Vec<((f32, f32), (f32, f32))>,
    /// `api`, `needs you`, `web · 2 subagents`, `infra · idle`.
    pub label: String,
    pub label_at: (f32, f32),
}

pub fn hash(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h
}

fn unit(h: u32) -> f32 {
    (h % 10_000) as f32 / 10_000.0
}

/// Centre of the `rank`th busiest of `n` repos, in units of one cell from the canvas centre: the
/// busiest in the middle, the rest on rings of up to six around it.
pub fn slot(rank: usize, n: usize) -> (f32, f32) {
    if rank == 0 {
        return (0.0, 0.0);
    }
    let ring = 1 + (rank - 1) / 6;
    let on_ring = (n - 1 - (ring - 1) * 6).min(6);
    let a = std::f32::consts::TAU * ((rank - 1) % 6) as f32 / on_ring as f32 - std::f32::consts::FRAC_PI_2 + 0.5 * ring as f32;
    (ring as f32 * a.cos(), ring as f32 * a.sin())
}

/// `rate` is each tab's tool calls in the last few minutes; `w`×`h` is the canvas.
pub fn sky(fleet: &Fleet, rate: &BTreeMap<String, u32>, w: f32, h: f32) -> Vec<Constellation> {
    let mut repos: BTreeMap<&str, Vec<(&String, &crate::fleet::Agent)>> = BTreeMap::new();
    for (s, a) in &fleet.agents {
        repos.entry(a.name.as_str()).or_default().push((s, a));
    }
    let mut ranked: Vec<_> = repos.into_iter().collect();
    let calls = |agents: &[(&String, &crate::fleet::Agent)]| agents.iter().map(|(s, _)| rate.get(*s).copied().unwrap_or(0)).sum::<u32>();
    let live = |agents: &[(&String, &crate::fleet::Agent)]| agents.iter().filter(|(_, a)| !matches!(a.state, State::Done | State::Failed)).count();
    ranked.sort_by(|(ra, a), (rb, b)| calls(b).cmp(&calls(a)).then(live(b).cmp(&live(a))).then(ra.cmp(rb)));
    let n = ranked.len().max(1);
    let rings = (n - 1).div_ceil(6);
    let (cw, ch) = (w / (2 * rings + 1) as f32, h / (2 * rings + 1) as f32);
    let busiest = rate.values().copied().max().unwrap_or(0).max(1) as f32;
    ranked
        .into_iter()
        .enumerate()
        .map(|(i, (repo, agents))| {
            let (sx, sy) = slot(i, n);
            let (cx, cy) = (w / 2.0 + sx * cw - cw / 2.0, h / 2.0 + sy * ch - ch / 2.0);
            let stars: Vec<Star> = agents
                .iter()
                .enumerate()
                .map(|(j, (s, a))| {
                    let hh = hash(&format!("{repo}/{j}"));
                    let r = rate.get(*s).copied().unwrap_or(0) as f32 / busiest;
                    let active = a.state != State::Done && a.state != State::Failed;
                    Star {
                        surface: (*s).clone(),
                        x: cx + cw * (0.15 + 0.7 * unit(hh)),
                        y: cy + ch * (0.2 + 0.6 * unit(hh.rotate_left(13))),
                        radius: 1.0 + 3.0 * if active { r.max(0.15) } else { 0.0 },
                        brightness: if active { 0.55 + 0.45 * r } else { 0.35 },
                        needs_you: a.state == State::NeedsYou,
                        moons: if active { a.subagents } else { 0 },
                    }
                })
                .collect();
            let lines = stars.windows(2).map(|p| ((p[0].x, p[0].y), (p[1].x, p[1].y))).collect();
            let moons: u32 = stars.iter().map(|s| s.moons).sum();
            let label = if stars.iter().any(|s| s.needs_you) {
                "needs you".to_owned()
            } else if moons > 0 {
                format!("{repo} · {moons} subagent{}", if moons == 1 { "" } else { "s" })
            } else if agents.iter().all(|(_, a)| matches!(a.state, State::Done | State::Failed)) {
                format!("{repo} · idle")
            } else {
                repo.to_owned()
            };
            let top = stars.iter().fold((f32::MAX, f32::MAX), |(x, y), s| (x.min(s.x), y.min(s.y)));
            Constellation { repo: repo.to_owned(), lines, label, label_at: (top.0, (top.1 - 8.0).max(cy + 8.0)), stars }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet::Ev;

    fn fleet() -> Fleet {
        let mut f = Fleet::default();
        let ev = |surface, kind, cwd| Ev { surface, kind, cwd: Some(cwd), ..Ev::default() };
        f.apply(&ev("a1", "permission_seen", "/w/api"), 0);
        f.apply(&ev("w1", "prompt_submit", "/w/web"), 0);
        for id in ["x", "y", "z"] {
            f.apply(&Ev { agent_id: Some(id), ..ev("w1", "subagent_start", "/w/web") }, 0);
        }
        f.apply(&Ev { agent_id: Some("z"), ..ev("w1", "subagent_stop", "/w/web") }, 0);
        f.apply(&ev("w2", "prompt_submit", "/w/web"), 0);
        f.apply(&ev("i1", "stop", "/w/infra"), 0);
        f
    }

    #[test]
    fn one_constellation_per_repo_with_mockup_labels() {
        let rate = BTreeMap::from([("w1".to_owned(), 20), ("w2".to_owned(), 5)]);
        let s = sky(&fleet(), &rate, 300.0, 150.0);
        let labels: Vec<_> = s.iter().map(|c| (c.repo.as_str(), c.label.as_str())).collect();
        assert_eq!(labels, [("web", "web · 2 subagents"), ("api", "needs you"), ("infra", "infra · idle")], "busiest first");
        let web = &s[0];
        assert_eq!((web.stars.len(), web.lines.len()), (2, 1));
        assert!(web.stars[0].radius > web.stars[1].radius && web.stars[0].brightness == 1.0, "busier is brighter");
        assert!(s[1].stars[0].needs_you);
        assert_eq!(s[2].stars[0].brightness, 0.35);
    }

    #[test]
    fn the_busiest_repo_sits_in_the_middle_and_moves_when_another_overtakes_it() {
        let centre = |c: &Constellation| {
            let n = c.stars.len() as f32;
            (c.stars.iter().map(|s| s.x).sum::<f32>() / n, c.stars.iter().map(|s| s.y).sum::<f32>() / n)
        };
        let near_middle = |c: &Constellation| {
            let (x, y) = centre(c);
            (x - 150.0).abs() < 50.0 && (y - 75.0).abs() < 25.0
        };
        let s = sky(&fleet(), &BTreeMap::from([("w1".to_owned(), 20)]), 300.0, 150.0);
        assert!(s[0].repo == "web" && near_middle(&s[0]));
        assert!(s[1..].iter().all(|c| !near_middle(c)));
        let s = sky(&fleet(), &BTreeMap::from([("w1".to_owned(), 2), ("a1".to_owned(), 9)]), 300.0, 150.0);
        assert!(s[0].repo == "api" && near_middle(&s[0]), "stars drift as activity changes");
    }

    #[test]
    fn rings_hold_six_and_spread_evenly() {
        assert_eq!(slot(0, 9), (0.0, 0.0));
        for (k, n) in [(1, 2), (3, 4), (6, 7), (7, 9)] {
            let (x, y) = slot(k, n);
            let ring = 1 + (k - 1) / 6;
            assert!(((x * x + y * y).sqrt() - ring as f32).abs() < 1e-4);
        }
        assert_ne!(slot(1, 3), slot(2, 3));
    }

    #[test]
    fn stars_stay_on_the_canvas_and_in_place() {
        let a = sky(&fleet(), &BTreeMap::new(), 300.0, 150.0);
        assert_eq!(a, sky(&fleet(), &BTreeMap::new(), 300.0, 150.0), "same fleet, same sky");
        for st in a.iter().flat_map(|c| &c.stars) {
            assert!((0.0..=300.0).contains(&st.x) && (0.0..=150.0).contains(&st.y));
        }
        assert!(sky(&Fleet::default(), &BTreeMap::new(), 300.0, 150.0).is_empty());
        let mut big = Fleet::default();
        for i in 0..9 {
            big.apply(&Ev { surface: &format!("s{i}"), kind: "prompt_submit", cwd: Some(&format!("/w/r{i}")), ..Ev::default() }, 0);
        }
        for st in sky(&big, &BTreeMap::new(), 600.0, 300.0).iter().flat_map(|c| &c.stars) {
            assert!((0.0..=600.0).contains(&st.x) && (0.0..=300.0).contains(&st.y), "{st:?}");
        }
    }
}
