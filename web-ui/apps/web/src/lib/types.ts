// API types — 与 Rust 后端 (src/models, src/web/handlers) 的 serde 结构对齐。
// 字段名一律 snake_case（后端未启用 camelCase rename）；后端 Option<T> 字段标为 T | null。

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

/** 搜索任务状态。对应后端 `web::SearchStatus`（Running/Done，PascalCase 序列化）。 */
export type SearchStatus = 'Running' | 'Done'

/** 单源搜索失败。对应后端 `web::SourceSearchError`。 */
export interface SearchSourceError {
  source_id: number
  source_name: string
  error: string
}

/** GET /api/search/{task_id} 轮询体。对应后端 `handlers::search::SearchStatusResponse`。 */
export interface SearchStatusResponse {
  status: SearchStatus
  total_sources: number
  done_sources: number
  results: SearchResult[]
  source_errors: SearchSourceError[]
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

/** 单章数据。对应后端 `models::Chapter`；content 在 TOC 接口常被省略。 */
export interface Chapter {
  url: string
  title: string
  /** 序号（从 1 开始），用于落盘文件名前缀补零排序。 */
  order: number
  content?: string
}

/** 任务状态。对应后端 `web::TaskStatus`（serde 默认 PascalCase）。 */
export type TaskStatus = 'Downloading' | 'Finished' | 'Failed' | 'Cancelled'

/** 下载任务信息。对应后端 `handlers::download::TaskInfo`。 */
export interface Task {
  id: number
  filename: string | null
  book_name: string | null
  /** 总章节数（book_resolved 后从 0 填到 N）。 */
  total_chapters: number
  /** 已完成的章节数（count，不是 index）。 */
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
 * 设置。对应后端 `config::AppConfig` 的可编辑子集；
 * 只读字段（version / theme_pref 等）由后端持有。
 */
export interface Settings {
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
export type ExportFormat = 'epub' | 'txt' | 'html' | 'pdf' | 'markdown'

/** startDownload 返回的任务标识。 */
export interface StartDownloadResult {
  task_id: number
}
