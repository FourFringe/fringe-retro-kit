//! Local web server for browsing exported map bundles.
//!
//! Serves three things from an export root: a dynamically generated table of contents across
//! every `<game>/<world>/manifest.json` found, a game-agnostic Leaflet viewer, and the static
//! tile/manifest files themselves (under `/b`). Serving over `http://localhost` keeps the
//! browser's `fetch()` of manifests working (which a `file://` page would block).

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use axum::{extract::State, response::Html, routing::get, Router};
use tower_http::services::ServeDir;

/// A single browsable world discovered under the export root.
struct Entry {
    game: String,
    world: String,
    title: String,
}

/// Run the server until interrupted.
pub async fn serve(root: PathBuf, port: u16) -> Result<()> {
    let state = Arc::new(root.clone());
    let app = Router::new()
        .route("/", get(toc))
        .route("/view", get(viewer))
        .nest_service("/b", ServeDir::new(root))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    println!("Serving maps at http://{addr}  (Ctrl-C to stop)");
    axum::serve(listener, app).await.context("running server")?;
    Ok(())
}

/// Static Leaflet viewer; it reads `?bundle=/game/world` and fetches that manifest + tiles.
async fn viewer() -> Html<&'static str> {
    Html(include_str!("web/viewer.html"))
}

/// Dynamic table of contents: every world with a `manifest.json` under the export root.
async fn toc(State(root): State<Arc<PathBuf>>) -> Html<String> {
    let mut entries = discover(&root);
    entries.sort_by(|a, b| (&a.game, &a.world).cmp(&(&b.game, &b.world)));

    let mut body = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>Fringe Retro Kit — Maps</title><style>\
         body{background:#0b0b12;color:#e6e6f0;font-family:system-ui,sans-serif;margin:0;padding:2rem}\
         h1{font-weight:600}ul{list-style:none;padding:0;max-width:40rem}\
         li{margin:.4rem 0}a{display:block;padding:.7rem 1rem;background:#1b1b2b;color:#cfe;\
         text-decoration:none;border-radius:6px}a:hover{background:#2a2a44}\
         .g{color:#8ab;font-size:.8rem}.empty{color:#99a}</style></head><body>\
         <h1>Exported maps</h1>",
    );
    if entries.is_empty() {
        body.push_str("<p class=\"empty\">No maps found. Bake one with <code>fringe-retro-map export</code>.</p>");
    } else {
        body.push_str("<ul>");
        for e in &entries {
            body.push_str(&format!(
                "<li><a href=\"/view?bundle=/{game}/{world}\">{title}<span class=\"g\"> — {game}/{world}</span></a></li>",
                game = html_escape(&e.game),
                world = html_escape(&e.world),
                title = html_escape(&e.title),
            ));
        }
        body.push_str("</ul>");
    }
    body.push_str("</body></html>");
    Html(body)
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
            let title =
                read_title(&manifest).unwrap_or_else(|| format!("{game_name}/{world_name}"));
            out.push(Entry {
                game: game_name.clone(),
                world: world_name,
                title,
            });
        }
    }
    out
}

fn read_title(manifest: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("title")
        .and_then(|t| t.as_str())
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
