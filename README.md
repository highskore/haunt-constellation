<div align="center">

<img src="docs/icon.png" width="96" alt="">

# Constellation

**Your agent fleet as a night sky.**

[![Haunt plugin](https://img.shields.io/badge/Haunt-plugin-D97757)](https://github.com/highskore/haunt-hackathon)
![Plugin API 0.5](https://img.shields.io/badge/plugin%20API-0.5-111)
![WASM component](https://img.shields.io/badge/WASM-component-654ff0?logo=webassembly&logoColor=white)
![MIT licence](https://img.shields.io/badge/licence-MIT-6b7280)

</div>

Run **Constellation** from the ⌘K palette and a window opens that you can move to a second display. Each repo is a constellation and each agent a star, joined by faint lines. A star grows bigger and brighter the more tool calls its agent made in the last 5 minutes; a finished agent dims. Running subagents sit around their star as moons. A star that needs you turns orange and pulses, and its constellation is labelled `needs you`. The others read `web · 2 subagents`, `infra · idle` or just the repo name.

The window redraws on every agent change and every 15 seconds while open. The busiest repo, by tool calls, sits in the middle and the others ring around it, so constellations change places as the work moves, drifting there over two and a half seconds (a jump under Reduce Motion). Within a constellation each star keeps its spot. A finished agent fades from the sky an hour after its last turn. The faint background dots are decoration and stand for nothing; turn them off with `background = false`.

## Settings

```toml
[plugins."dev.haunt.constellation"]
background = true   # faint background stars, for the look only
labels = true       # name each constellation
unverified = false  # also follow events without the tab's nonce
```

## Capabilities

| capability | why |
|---|---|
| `events` (session start, prompt, subagent, tool, permission request, notification, stop) | each agent's state, busyness and subagents; no event text is kept |
| `panel` | the window |
| `timer` | redraws every 15 seconds, only while the window is open |

Events without the tab's nonce are ignored unless `unverified` is on.

## Build and test

```sh
cargo build --release --target wasm32-wasip2   # or ./build.sh
cargo test                                     # unit tests
haunt plugin install .
```

`e2e/constellation.rs` runs the built plugin against a real Haunt core: copy it into a Haunt checkout's `cli/tests/` and run `HAUNT_PLUGINS_DIR=<this repo's parent> cargo test -p haunt-cli --test constellation`.

Needs Haunt with plugin API 0.5 (`api = "0.5"`).

## Licence

MIT, see `LICENSE`.
