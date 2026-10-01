//! `/patch.gfi` — the patch as a file the BROWSER carries, in both directions.
//!
//! A copy in and a copy out, not a second save semantics: `save_path` is untouched, so Ctrl-S keeps
//! meaning "overwrite the file this patch came from".

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::AppState;

/// The name the browser's Save dialog opens with: the open patch's own filename, or a default.
/// A quote or a control character would end the header field it sits in, so neither survives.
fn download_name(state: &AppState) -> String {
    let kept = |c: char| !c.is_control() && !"\"\\".contains(c);
    state
        .save_path
        .lock()
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().replace(|c| !kept(c), "_")))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "patch.gfi".into())
}

/// Pack the open patch under the graph lock, so the manifest and the workspace describe one moment.
fn pack(state: &AppState) -> Result<Vec<u8>, String> {
    let mount = state.mount();
    let tmp = crate::nonce_hex().and_then(|n| goofi_transport::scratch(state.iox.id(), &format!("export-{n}.gfi")))?;
    let packed = {
        let g = state.graph.lock();
        let extra = crate::bundled_custom(&g, &state.custom);
        goofi_graph::archive::write_gfi(&tmp, &g.serialize(), &mount, &extra)
    }
    .and_then(|()| std::fs::read(&tmp).map_err(|e| format!("{}: {e}", tmp.display())));
    let _ = std::fs::remove_file(&tmp);
    packed
}

/// Run `work` off the async workers: a pack or a load holds the graph for seconds, and the sockets
/// must keep being polled meanwhile.
async fn blocking<T: Send + 'static>(state: &AppState, work: fn(&AppState) -> Result<T, String>) -> Result<T, String> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || work(&state)).await.unwrap_or_else(|e| Err(format!("the task died: {e}")))
}

/// `GET /patch.gfi` — pack the open patch and hand it over.
pub(crate) async fn download(State(state): State<AppState>) -> Response {
    match blocking(&state, pack).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", download_name(&state)),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// `POST /patch.gfi` — replace the open patch with the uploaded archive, through the real `load`
/// op. `adopt: false`, because the staged copy is deleted the moment the load returns.
pub(crate) async fn upload(State(state): State<AppState>, body: Bytes) -> Response {
    let tmp = match crate::nonce_hex().and_then(|n| goofi_transport::scratch(state.iox.id(), &format!("import-{n}.gfi"))) {
        Ok(tmp) => tmp,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    if let Err(e) = std::fs::write(&tmp, &body) {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {e}", tmp.display())).into_response();
    }
    let Some(path) = tmp.to_str().map(str::to_string) else {
        let _ = std::fs::remove_file(&tmp);
        return (StatusCode::INTERNAL_SERVER_ERROR, "the temp directory's name is not UTF-8\n").into_response();
    };
    let loader = state.clone();
    let load = tokio::task::spawn_blocking(move || loader.call("session load", json!({ "path": path, "adopt": false }), "upload"))
        .await
        .unwrap_or_else(|e| Err(format!("the load task died: {e}")));
    let _ = std::fs::remove_file(&tmp);

    match load {
        Ok(_) => (StatusCode::OK, "loaded\n").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, format!("{e}\n")).into_response(),
    }
}
