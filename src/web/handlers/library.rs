//! 书库列表 + 文件下载 + 删除。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::{Deserialize, Serialize};

use crate::utils::fs::sanitize_filename;

use super::super::SharedState;

#[derive(Serialize)]
pub(crate) struct LibraryEntry {
    filename: String,
    size: u64,
    modified: u64,
    ext: String,
}

#[derive(Deserialize)]
pub(crate) struct LibraryQuery {
    pub ext: Option<String>,
}

pub async fn library_list(
    Query(q): Query<LibraryQuery>,
    State(state): State<SharedState>,
) -> Json<Vec<LibraryEntry>> {
    let dir = &state.download_path;
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !["epub", "txt", "html", "zip", "pdf"].contains(&ext.as_str()) {
                continue;
            }
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            entries.push(LibraryEntry {
                filename: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string(),
                size,
                modified,
                ext,
            });
        }
    }
    if let Some(ext) = &q.ext {
        entries.retain(|f| f.ext.eq_ignore_ascii_case(ext));
    }
    entries.sort_by_key(|b| std::cmp::Reverse(b.modified));
    Json(entries)
}

pub async fn library_delete(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<&'static str, (StatusCode, String)> {
    let safe = sanitize_filename(&filename);
    let path = state.download_path.join(&safe);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "文件未找到".to_string()));
    }
    std::fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok("已删除")
}

pub async fn file_download(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let safe = sanitize_filename(&filename);
    let path = state.download_path.join(&safe);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "文件未找到".to_string()));
    }
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let content_type = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "epub" => "application/epub+zip",
        "txt" => "text/plain; charset=utf-8",
        "html" | "zip" => "application/zip",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    };

    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}
