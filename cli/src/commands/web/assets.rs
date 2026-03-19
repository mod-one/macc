use super::WebState;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{self, HeaderValue};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
#[cfg(not(any(test, clippy)))]
use rust_embed::RustEmbed;
use std::ffi::OsStr;
use std::path::Path;

#[cfg(not(any(test, clippy)))]
#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct EmbeddedWebAssets;

#[cfg(not(any(test, clippy)))]
fn web_asset(path: &str) -> Option<Vec<u8>> {
    EmbeddedWebAssets::get(path).map(|asset| asset.data.into_owned())
}

#[cfg(test)]
pub(super) fn web_asset_paths() -> impl Iterator<Item = &'static str> {
    TEST_WEB_ASSETS.iter().map(|(path, _)| *path)
}

#[cfg(any(test, clippy))]
fn web_asset(path: &str) -> Option<Vec<u8>> {
    TEST_WEB_ASSETS
        .iter()
        .find_map(|(candidate, body)| (*candidate == path).then(|| body.as_bytes().to_vec()))
}

#[cfg(any(test, clippy))]
const TEST_WEB_ASSETS: &[(&str, &str)] = &[
    (
        "index.html",
        "<!doctype html><html><body>macc web</body></html>",
    ),
    ("assets/app.js", "console.log('macc');"),
];

pub(super) async fn spa_handler(State(state): State<WebState>, uri: Uri) -> Response {
    let asset_path = uri.path().trim_start_matches('/');
    let asset_path = if asset_path.is_empty() {
        "index.html"
    } else {
        asset_path
    };

    asset_response(&state, asset_path)
        .or_else(|| asset_response(&state, "index.html"))
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn asset_response(state: &WebState, path: &str) -> Option<Response> {
    let asset = match state.assets_mode {
        macc_core::config::WebAssetsMode::Dist => dist_asset(path, &state.paths.root),
        macc_core::config::WebAssetsMode::Embedded => web_asset(path),
    }?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(asset));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).ok()?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control_header(path)),
    );
    Some(response)
}

fn dist_asset(path: &str, root: &Path) -> Option<Vec<u8>> {
    std::fs::read(root.join("web").join("dist").join(path)).ok()
}

fn cache_control_header(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(OsStr::to_str) {
        Some("html") => "no-cache",
        _ => "public, max-age=31536000, immutable",
    }
}
