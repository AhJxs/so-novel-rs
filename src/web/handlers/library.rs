//! 书库列表 + 文件下载 + 删除。
//!
//! Phase 3.5：白名单 / `LibraryEntry` / `list_library_entries` / `open_download_file`
//! 全部搬到 [`crate::core::library`]；这里只做 HTTP 层的取参 + 状态码映射。
//!
//! Phase 4.x：handler 错误统一走 [`crate::web::error::WebError`]，响应 body
//! 经 [`crate::i18n::ts_for_locale`] 按请求 locale 翻译。成功 body（删除确认）
//! 同样是按 locale 翻译的 localized 字符串。

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;

use crate::core::library::{
    LibraryEntry, OpenFileError, list_library_entries, open_download_file, safe_file_path,
};
use crate::i18n::ts_for_locale;
use crate::utils::fs::sanitize_filename;
use crate::web::SharedState;
use crate::web::error::WebError;
use crate::web::locale::Locale;

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

/// `DELETE /api/library/:filename` — 删除本地下载文件。
///
/// 成功响应：localized "Deleted" / "已删除" / "已刪除" 字符串（plain text body）。
pub async fn library_delete(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<String, WebError> {
    let path = safe_file_path(&state.download_path, &filename)?;
    std::fs::remove_file(&path)?;
    Ok(ts_for_locale(locale, "WebErrors.library_deleted"))
}

/// `GET /api/files/:filename` — 下载书库文件（binary body）。
///
/// 错误经 `WebError` 渲染（status 400/404/500 + JSON envelope）。
pub async fn file_download(
    State(state): State<SharedState>,
    Path(filename): Path<String>,
) -> Result<Response, WebError> {
    let (bytes, content_type) = open_download_file(&state.download_path, &filename).await?;
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

// ── WebError 自动装箱 ──────────────────────────────────────────
//
// `OpenFileError` → `WebError`：NotFound → 404 + WebErrors.not_found 翻译；
// Io → 500 + WebErrors.internal 翻译（避免泄漏内部路径）。
impl From<OpenFileError> for WebError {
    fn from(e: OpenFileError) -> Self {
        match e {
            OpenFileError::NotFound => Self::NotFound(""),
            OpenFileError::Io(_) => Self::Internal("io_error"),
        }
    }
}
