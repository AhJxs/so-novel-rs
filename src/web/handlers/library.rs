//! 书库列表 + 文件下载 + 删除。
//!
//! Phase 3.5：白名单 / `LibraryEntry` / `list_library_entries` / `open_download_file`
//! 全部搬到 [`crate::core::library`]；这里只做 HTTP 层的取参 + 状态码映射。

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

use crate::core::library::{
    LibraryEntry, OpenFileError, list_library_entries, open_download_file, safe_file_path,
};
use crate::utils::fs::sanitize_filename;

use super::super::SharedState;

#[derive(Deserialize)]
pub struct LibraryQuery {
    pub ext: Option<String>,
}

pub async fn library_list(
    Query(q): Query<LibraryQuery>,
    State(state): State<SharedState>,
) -> Json<Vec<LibraryEntry>> {
    let mut entries = list_library_entries(&state.download_path);
    if let Some(ext) = &q.ext {
        entries.retain(|f| f.ext.eq_ignore_ascii_case(ext));
    }
    Json(entries)
}

pub async fn library_delete(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<&'static str, (StatusCode, String)> {
    let path = safe_file_path(&state.download_path, &filename).map_err(map_open_err)?;
    std::fs::remove_file(&path).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok("已删除")
}

pub async fn file_download(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<Response, (StatusCode, String)> {
    let (bytes, content_type) = open_download_file(&state.download_path, &filename)
        .await
        .map_err(map_open_err)?;
    // `Content-Disposition` 用 sanitize 后的文件名 —— 避免原始输入里的 `"` / `;` / `\r\n`
    // 破坏 header 格式（HTTP header injection）。
    let safe = sanitize_filename(&filename);
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

/// `OpenFileError` → `(StatusCode, String)` 映射。`NotFound` → 404，其它 → 500。
///
/// Phase 3.8 web 内部清理阶段会进一步抽到 `web/error.rs` 的统一 mapping 里（届时
/// `delete` / `download` / `tasks::cancel` 等 handler 共用），这里先局部抽出来避免
/// `library_delete` + `file_download` 两处重复 match。
fn map_open_err(e: OpenFileError) -> (StatusCode, String) {
    match e {
        OpenFileError::NotFound => (StatusCode::NOT_FOUND, e.to_string()),
        OpenFileError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
    }
}
