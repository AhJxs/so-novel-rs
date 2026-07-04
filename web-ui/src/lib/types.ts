// API types — 与 Rust 后端 (src/models, src/web/handlers) 的 serde 序列化结构对齐。
// 后端结构体未启用 camelCase rename，字段名一律 snake_case，故前端保持一致。
// 凡后端 #[serde(skip_serializing_if = "Option::is_none")] 或 Option<T> 的字段，
// 此处均标为 T | null（serde 默认把 None 序列化为 null）。

/** 单条搜索结果。对应后端 `models::SearchResult`。 */
export interface SearchResult {
  source_id: number
  source_name: string
  url: string
  book_name: string
  author: string | null
  intro: string | null
  category: string | null
  latest_chapter: string | null
  last_update_time: string | null
  status: string | null
  word_count: string | null
}

/** 详情页解析后的书籍数据。对应后端 `models::Book`。 */
export interface Book {
  url: string
  book_name: string
  author: string
  intro: string | null
  category: string | null
  cover_url: string | null
  latest_chapter: string | null
  latest_chapter_url: string | null
  last_update_time: string | null
  status: string | null
  /** 书源语言（如 zh-CN、zh-TW），由解析时从 rule.language 填入。 */
  language: string
}

/** 单章数据（目录/进度用）。对应后端 `models::Chapter`，但 content 在 TOC 接口常被省略。 */
export interface Chapter {
  url: string
  title: string
  /** 序号（从 1 开始），用于落盘文件名前缀补零排序。 */
  order: number
  content?: string
}

/**
 * 任务状态。对应后端 `web::TaskStatus` 枚举（serde 默认 PascalCase 序列化为
 * "Downloading" | "Finished" | "Failed" | "Cancelled"）。
 */
export type TaskStatus = 'Downloading' | 'Finished' | 'Failed' | 'Cancelled'

/** 下载任务信息。对应后端 `handlers::download::TaskInfo`。 */
export interface Task {
  id: number
  filename: string | null
  book_name: string | null
  /** 总章节数（book_resolved 后从 0 填到 N）。 */
  total_chapters: number
  /** 已完成的章节数（count，不是 index；并发场景下单调递增，progress bar 平滑）。 */
  current_chapter: number
  /** 已失败章节数（与 GPUI DownloadTask.failed 同语义）。 */
  failed: number
  status: TaskStatus
  started_at_unix: number
  finished_at_unix: number | null
}

/** 书库文件条目。对应后端 `handlers::library::LibraryEntry`。 */
export interface LibraryFile {
  filename: string
  size: number
  modified: number
  ext: string
}

/** 书源信息。对应后端 `handlers::misc::SourceInfo`。 */
export interface Source {
  id: number
  name: string
  url: string
  enabled: boolean
}

/** 书源测速结果。对应后端 `handlers::misc::SourceTestResult`。 */
export interface SourceTestResult {
  ok: boolean
  latency_ms: number
  error: string | null
}

/**
 * 设置。对应后端 `config::AppConfig` 的可编辑子集。
 * GET /api/settings 返回完整 AppConfig；PUT /api/settings 接受部分字段（SettingsUpdate）。
 * 这里只列前端会读写的字段；只读字段（version/theme_pref 等）由后端持有。
 */
export interface Settings {
  /** 应用语言。对应后端 config::Language。 */
  language?: 'SimplifiedChinese' | 'TraditionalChinese' | 'English'
  proxy_enabled: boolean
  proxy_host: string
  proxy_port: number
  concurrency: number | null
  max_retries: number
  enable_retry: boolean
  min_interval: number
  max_interval: number
  cf_bypass: string
  download_path: string
  ext_name: ExportFormat
  txt_encoding: string
  search_filter: boolean
}

/** 导出文件格式。对应后端 `config::ExportFormat`（serde 序列化为小写变体名）。 */
export type ExportFormat = 'epub' | 'txt' | 'html' | 'pdf'

/** startDownload 返回的任务标识。 */
export interface StartDownloadResult {
  task_id: number
}

// ─── SSE 事件类型 ─────────────────────────────────────────────
// 后端搜索与下载均走 SSE（axum::response::Sse），前端用 fetch + ReadableStream 解析。

/** 搜索 SSE 单条事件 data。对应后端 `handlers::search::SearchEvent`。 */
export interface SearchStreamEvent {
  source_id: number
  source_name: string
  results: SearchResult[]
  error: string | null
}

/** 搜索 SSE 结束事件 data。对应后端 `handlers::search::SearchDoneEvent`。 */
export interface SearchDoneEvent {
  total: number
}

/**
 * 下载进度 SSE 事件 data。对应后端 `handlers::download::ProgressEvent`
 * （字段名 type，其余按 kind 按需出现）。
 */
export interface DownloadProgressEvent {
  type:
    | 'book_resolved'
    | 'chapter_done'
    | 'chapter_failed'
    | 'finished'
    | 'cancelled'
    | 'failed'
  index?: number
  title?: string
  task_id?: number
  filename?: string
  reason?: string
  total?: number
  book_name?: string
}
