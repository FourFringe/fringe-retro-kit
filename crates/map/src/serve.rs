//! Local web server for browsing exported map bundles.
//!
//! Serves three things from an export root: a dynamically generated table of contents across
//! every `<game>/<world>/manifest.json` found, a game-agnostic Leaflet viewer, and the static
//! tile/manifest files themselves (under `/b`). Serving over `http://localhost` keeps the
//! browser's `fetch()` of manifests working (which a `file://` page would block).

use std::{
    convert::Infallible,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::{header, HeaderValue},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse,
    },
    routing::get,
    Json, Router,
};
use futures_core::Stream;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;

use crate::{config::Config, tilemap, ultima1, ultima2, ultima3, ultima4, ultima5, wasteland};

/// Shared server state: where bundles live, plus the config for resolving each game's save.
struct AppState {
    root: PathBuf,
    config: Config,
}

/// A single browsable world discovered under the export root.
struct Entry {
    game: String,
    world: String,
    title: String,
    kind: String,
    group: String,
}

/// Run the server until interrupted. Leaflet is served locally, so no internet is required.
pub async fn serve(root: PathBuf, port: u16, open: bool, config: Config) -> Result<()> {
    let state = Arc::new(AppState {
        root: root.clone(),
        config,
    });
    let app = Router::new()
        .route("/", get(toc))
        .route("/view", get(viewer))
        .route("/leaflet.js", get(leaflet_js))
        .route("/leaflet.css", get(leaflet_css))
        .route("/api/position", get(position))
        .route("/api/position/stream", get(position_stream))
        .nest_service("/b", ServeDir::new(root))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    let url = format!("http://{addr}");
    println!("Serving maps at {url}  (Ctrl-C to stop)");
    if open {
        let _ = open::that(&url);
    }
    axum::serve(listener, app).await.context("running server")?;
    Ok(())
}

/// Static Leaflet viewer; it reads `?bundle=/game/world` and fetches that manifest + tiles.
async fn viewer() -> Html<&'static str> {
    Html(include_str!("web/viewer.html"))
}

/// Vendored Leaflet script (BSD-2-Clause), served locally for offline use.
async fn leaflet_js() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/javascript"),
        )],
        include_str!("web/vendor/leaflet.js"),
    )
}

/// Vendored Leaflet stylesheet, served locally for offline use.
async fn leaflet_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static("text/css"))],
        include_str!("web/vendor/leaflet.css"),
    )
}

/// Query for the player-position endpoint: which `game`, and which `world` is being viewed (the
/// marker is only shown on worlds that make sense for the save's position).
#[derive(Deserialize)]
struct PosQuery {
    game: String,
    #[serde(default)]
    world: String,
}

/// Whether a `(game, world)` pair should display a live "you are here" marker.
///
/// Ultima I has a single overworld, so any world qualifies. Ultima II records an overworld
/// position but not *which* world/era it belongs to, so we show it on the three Earth overworlds
/// (`MAPX20`/`MAPX30`/`MAPX40`) until the save's map-selector byte is reverse-engineered; towns
/// and other worlds get no marker.
fn supports_position(game: &str, world: &str) -> bool {
    match game {
        "ultima1" => true,
        "ultima2" => matches!(world, "mapx20" | "mapx30" | "mapx40"),
        "ultima3" => world == "sosaria",
        "ultima4" => world == "britannia",
        "ultima5" => matches!(world, "britannia" | "underworld"),
        // The party can be on any Wasteland map; the marker only shows on the one it's on.
        "wasteland" => world.starts_with("map"),
        _ => false,
    }
}

/// Read the current party position (in tiles) for a game from its save directory. `world` is the
/// world being viewed, so games with more than one positionable world (Ultima V's surface and
/// Underworld) only return a position when the save is on that world.
fn read_position(game: &str, world: &str, dir: &Path) -> Option<(u32, u32)> {
    match game {
        "ultima1" => ultima1::player_position(dir).ok().flatten(),
        "ultima2" => ultima2::player_position(dir).ok().flatten(),
        "ultima3" => ultima3::player_position(dir).ok().flatten(),
        "ultima4" => ultima4::player_position(dir).ok().flatten(),
        "ultima5" => ultima5::player_position(dir)
            .ok()
            .flatten()
            .and_then(|(underworld, x, y)| {
                let want = if underworld {
                    "underworld"
                } else {
                    "britannia"
                };
                (world == want).then_some((x, y))
            }),
        // The Wasteland save records the current map id; show the marker only when the viewed
        // world is that map. Worlds are numbered by the game's own map id (0-based), so the
        // savegame's `map` matches the `map{id}` world directly.
        "wasteland" => wasteland::player_position(dir)
            .ok()
            .flatten()
            .and_then(|(map, x, y)| (world == format!("map{map}")).then_some((x, y))),
        _ => None,
    }
}

/// The party's current position in image **pixel** coordinates, if a save is available.
/// `supported` tells the viewer whether this game has live-position support at all, so it can
/// avoid opening a position stream (and holding an idle connection) for games that don't.
#[derive(Serialize)]
struct PositionResp {
    supported: bool,
    px: Option<u32>,
    py: Option<u32>,
}

/// Read the current party position from the game's save, returning it in image pixel coordinates
/// so the viewer can place a marker with no game knowledge. `supported` reflects whether this
/// world shows a marker at all (see [`supports_position`]).
async fn position(
    State(app): State<Arc<AppState>>,
    Query(q): Query<PosQuery>,
) -> Json<PositionResp> {
    if !supports_position(&q.game, &q.world) {
        return Json(PositionResp {
            supported: false,
            px: None,
            py: None,
        });
    }
    let pos = app
        .config
        .game_input_dir(&q.game)
        .and_then(|dir| read_position(&q.game, &q.world, &dir));
    let (px, py) = match pos {
        Some(p) => {
            let (px, py) = tile_center_px(p);
            (Some(px), Some(py))
        }
        None => (None, None),
    };
    Json(PositionResp {
        supported: true,
        px,
        py,
    })
}

/// Live player position: an SSE stream that pushes the party position whenever the save file
/// changes (the server watches the game directory), plus the current position on connect.
async fn position_stream(
    State(app): State<Arc<AppState>>,
    Query(q): Query<PosQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let dir = supports_position(&q.game, &q.world)
        .then(|| app.config.game_input_dir(&q.game))
        .flatten();
    let game = q.game;
    let world = q.world;

    let stream = async_stream::stream! {
        // Without a resolvable save directory there's nothing to watch; hold the connection
        // open but idle so the browser doesn't reconnect in a tight loop.
        let Some(dir) = dir else {
            std::future::pending::<()>().await;
            return;
        };

        // Bridge notify's callback thread to this async task via an unbounded channel.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let watcher = notify::recommended_watcher(move |_res| {
            let _ = tx.send(());
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(_) => {
                std::future::pending::<()>().await;
                return;
            }
        };
        // Watch the directory (saves may be written via atomic replace, not in-place).
        let _ = watcher.watch(&dir, RecursiveMode::NonRecursive);

        // Emit the current position immediately, then only on change.
        let mut last: Option<(u32, u32)> = None;
        if let Some(pos) = read_position(&game, &world, &dir) {
            last = Some(pos);
            yield Ok::<_, Infallible>(position_event(pos));
        }
        while rx.recv().await.is_some() {
            // Debounce a burst of filesystem events into a single read.
            tokio::time::sleep(Duration::from_millis(150)).await;
            while rx.try_recv().is_ok() {}
            if let Some(pos) = read_position(&game, &world, &dir) {
                if Some(pos) != last {
                    last = Some(pos);
                    yield Ok::<_, Infallible>(position_event(pos));
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// A tile position as a `data:` SSE event carrying `{px, py}` image pixel coordinates.
fn position_event(pos: (u32, u32)) -> Event {
    let (px, py) = tile_center_px(pos);
    Event::default().data(serde_json::json!({ "px": px, "py": py }).to_string())
}

/// Convert a tile position to image pixel coordinates (the tile's centre).
fn tile_center_px(pos: (u32, u32)) -> (u32, u32) {
    let half = tilemap::TILE_SIZE / 2;
    (
        pos.0 * tilemap::TILE_SIZE + half,
        pos.1 * tilemap::TILE_SIZE + half,
    )
}

/// Dynamic table of contents: every world with a `manifest.json`, grouped by game and then by
/// region — an overworld with its towns/castles nested beneath it.
async fn toc(State(app): State<Arc<AppState>>) -> Html<String> {
    let entries = discover(&app.root);

    let mut body = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Fringe Retro Kit — Maps</title><style>\
         body{background:#0b0b12;color:#e6e6f0;font-family:system-ui,sans-serif;margin:0;padding:2rem}\
         h1{font-weight:600}\
         details{max-width:44rem}summary{cursor:pointer;user-select:none}\
         summary.game{font-weight:600;font-size:1.15rem;margin:1.5rem 0 .5rem;\
         border-bottom:1px solid #2a2a44;padding-bottom:.3rem}\
         summary.subs{margin:.35rem 0;padding:.55rem .9rem;background:#161622;color:#9cf;\
         border-radius:6px;font-size:.85rem}summary.subs:hover{background:#23233c}\
         details.region{margin-left:1.6rem}\
         ul{list-style:none;padding:0;max-width:44rem}li{margin:.35rem 0}\
         a{display:flex;align-items:center;gap:.6rem;padding:.6rem .9rem;background:#1b1b2b;\
         color:#cfe;text-decoration:none;border-radius:6px}a:hover{background:#2a2a44}\
         li.sub a{background:#161622}\
         .name{flex:1}.g{color:#8ab;font-size:.75rem}\
         .count{color:#8ab;font-size:.75rem;font-weight:400;margin-left:.4rem}\
         .badge{font-size:.62rem;text-transform:uppercase;letter-spacing:.05em;padding:.15rem .45rem;\
         border-radius:4px;background:#2a2a44;color:#9df}\
         .badge.overworld{background:#3a3320;color:#ffd24a}.empty{color:#99a}</style></head><body>\
         <h1>Exported maps</h1>",
    );

    if entries.is_empty() {
        body.push_str("<p class=\"empty\">No maps found. Bake one with <code>fringe-retro-map export</code>.</p></body></html>");
        return Html(body);
    }

    let mut games: Vec<&str> = entries.iter().map(|e| e.game.as_str()).collect();
    games.sort_unstable();
    games.dedup();

    for game in games {
        let mut group: Vec<&Entry> = entries.iter().filter(|e| e.game == game).collect();
        // Region key, then overworld before its sub-maps, then world id.
        group.sort_by(|a, b| {
            (&a.group, a.kind != "overworld", &a.world).cmp(&(
                &b.group,
                b.kind != "overworld",
                &b.world,
            ))
        });
        // The game heading is the shared title prefix (e.g. "Ultima II — Towne Linda" → "Ultima II").
        let header = group
            .first()
            .map(|e| e.title.split(" — ").next().unwrap_or(&e.title))
            .filter(|s| !s.is_empty())
            .unwrap_or(game)
            .to_owned();
        // Each game is a collapsible section; its overworlds show directly, and the sub-maps of
        // each overworld nest in their own collapsed group so long town lists don't flood the page.
        body.push_str(&format!(
            "<details class=\"game\"><summary class=\"game\">{header}\
             <span class=\"count\">{n} map{s}</span></summary><ul>",
            header = html_escape(&header),
            n = group.len(),
            s = if group.len() == 1 { "" } else { "s" },
        ));
        let mut i = 0;
        while i < group.len() {
            let region = &group[i].group;
            let mut j = i;
            while j < group.len() && &group[j].group == region {
                j += 1;
            }
            let (overworlds, subs): (Vec<&Entry>, Vec<&Entry>) =
                group[i..j].iter().partition(|e| e.kind == "overworld");
            for e in overworlds {
                push_toc_item(&mut body, e, false);
            }
            if !subs.is_empty() {
                body.push_str(&format!(
                    "<li><details class=\"region\"><summary class=\"subs\">\
                     {n} sub-map{s}</summary><ul>",
                    n = subs.len(),
                    s = if subs.len() == 1 { "" } else { "s" },
                ));
                for e in subs {
                    push_toc_item(&mut body, e, true);
                }
                body.push_str("</ul></details></li>");
            }
            i = j;
        }
        body.push_str("</ul></details>");
    }
    body.push_str("</body></html>");
    Html(body)
}

/// Append one world as a table-of-contents list item link; `sub` marks a nested sub-map.
fn push_toc_item(body: &mut String, e: &Entry, sub: bool) {
    // Drop the redundant game prefix now that the world sits under a game heading.
    let name = e
        .title
        .split_once(" — ")
        .map_or(e.title.as_str(), |(_, r)| r);
    let badge = if e.kind.is_empty() {
        String::new()
    } else {
        format!(
            "<span class=\"badge {kind}\">{kind}</span>",
            kind = html_escape(&e.kind)
        )
    };
    body.push_str(&format!(
        "<li{class}><a href=\"/view?bundle=/{game}/{world}\">\
         <span class=\"name\">{name}</span>{badge}<span class=\"g\">{world}</span></a></li>",
        class = if sub { " class=\"sub\"" } else { "" },
        game = html_escape(&e.game),
        world = html_escape(&e.world),
        name = html_escape(name),
    ));
}

/// Scan `<root>/<game>/<world>/manifest.json`, reading each world's title.
fn discover(root: &PathBuf) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(games) = std::fs::read_dir(root) else {
        return out;
    };
    for game in games.flatten().filter(|e| e.path().is_dir()) {
        let game_name = game.file_name().to_string_lossy().into_owned();
        let Ok(worlds) = std::fs::read_dir(game.path()) else {
            continue;
        };
        for world in worlds.flatten().filter(|e| e.path().is_dir()) {
            let manifest = world.path().join("manifest.json");
            if !manifest.is_file() {
                continue;
            }
            let world_name = world.file_name().to_string_lossy().into_owned();
            let meta = read_manifest(&manifest);
            let field = |key: &str| meta.as_ref().and_then(|m| manifest_str(m, key));
            let title = field("title").unwrap_or_else(|| format!("{game_name}/{world_name}"));
            let kind = field("kind").unwrap_or_default();
            // Ungrouped worlds each form their own group so they still list cleanly.
            let group = field("group").unwrap_or_else(|| world_name.clone());
            out.push(Entry {
                game: game_name.clone(),
                world: world_name,
                title,
                kind,
                group,
            });
        }
    }
    out
}

/// Parse a bundle's `manifest.json`, if present and valid.
fn read_manifest(manifest: &std::path::Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(manifest).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read a non-empty string field from a parsed manifest.
fn manifest_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_finds_worlds_with_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("ultima1").join("overworld");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(
            world.join("manifest.json"),
            r#"{"title":"Ultima I — Sosaria"}"#,
        )
        .unwrap();
        // A directory without a manifest is ignored.
        std::fs::create_dir_all(dir.path().join("ultima2").join("empty")).unwrap();

        let found = discover(&dir.path().to_path_buf());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].game, "ultima1");
        assert_eq!(found[0].world, "overworld");
        assert_eq!(found[0].title, "Ultima I — Sosaria");
    }

    #[test]
    fn html_escape_escapes() {
        assert_eq!(html_escape("a<b>&\"c"), "a&lt;b&gt;&amp;&quot;c");
    }
}
