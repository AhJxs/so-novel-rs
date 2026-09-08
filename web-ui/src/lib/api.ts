// API 客户端 —— 所有 fetch 调用集中在此。base URL 固定 /api，不硬编码 host:port。
//
// 后端契约来源：src/web/routes.rs + src/web/handlers/*.rs。
// - 普通 JSON 接口用 apiFetch<T>。
// - 搜索（GET /api/search）与下载（POST /api/download）是 SSE 流，用 fetch + ReadableStream 手动解析，
//   通过 onEvent 回调推送（浏览器原生 EventSource 不支持 POST，且 GET SSE 也需对 event name 分发）。

import type {
  Book,
  Chapter,
  ExportFormat,
  LibraryFile,
  SearchStatusResponse,
  Settings,
  Source,
  SourceTestResult,
  StartDownloadResult,
  Task,
} from './types'

const API_BASE = '/api'

/**
 * 后端错误响应（见 `src/web/error.rs::ErrorEnvelope`）。
 * 形态: `{ "error": { "code": "bad_request", "code_id": "3004", "message": "..." } }`
 * - `code`：`WebErrorKind` snake_case（错误大类）
 * - `code_id`：稳定数字码（前端 dispatch 用，e.g. `3005` → download_path_not_dir）
 * - `message`：按 Accept-Locale 翻译的本地化文案，直接展示给用户
 */
interface BackendErrorBody {
  error: {
    code: string
    code_id: string
    message: string
  }
}

/**
 * API 错误。扩展 `Error` 加 `code` / `codeId` —— 前端 dispatch 按这两个稳定字段，
 * 不要按 `message` 字符串匹配（i18n 后 message 是 localized，无法 substring 匹配）。
 *
 * - `code`：`WebErrorKind` snake_case 短码（`bad_request` 等大类），供日志聚合
 * - `codeId`：稳定数字码字符串（`"3005"` 等），供前端按业务类型 dispatch
 * - `message`：localized user-facing 文案
 */
export class ApiError extends Error {
  /** HTTP 状态码（4xx / 5xx），便于 UI 区分客户端错误 / 服务端错误 */
  public readonly status: number
  /** WebErrorKind snake_case 短码（错误大类） */
  public readonly code: string
  /** 业务层稳定数字码（前端 dispatch 用） */
  public readonly codeId: string

  constructor(message: string, opts: { status: number; code: string; codeId: string }) {
    super(message)
    this.name = 'ApiError'
    this.status = opts.status
    this.code = opts.code
    this.codeId = opts.codeId
  }
}

/**
 * 从非-OK Response 抽出 ApiError。优先按后端 JSON envelope 解析；解析失败
 * （非 JSON body / 旧后端 / 反代截断）降级成 `HTTP {status}` + 空 code/codeId。
 */
async function toApiError(res: Response): Promise<ApiError> {
  const text = await res.text().catch(() => res.statusText)
  // 尝试解析后端 envelope —— 任何一步失败都降级
  let parsed: BackendErrorBody | null = null
  try {
    if (text) parsed = JSON.parse(text) as BackendErrorBody
  } catch {
    parsed = null
  }
  if (parsed?.error?.message) {
    return new ApiError(parsed.error.message, {
      status: res.status,
      code: parsed.error.code ?? '',
      codeId: parsed.error.code_id ?? '',
    })
  }
  // 降级：非 JSON / envelope 缺字段 —— 用 HTTP status + 原始文本
  return new ApiError(text || `HTTP ${res.status}`, {
    status: res.status,
    code: '',
    codeId: '',
  })
}

/**
 * JSON fetch 封装。统一拼 /api 前缀，处理错误与 JSON 反序列化。
 * 对返回 void 的接口，泛型传 void，函数返回 Promise<void>（仍会消费响应体）。
 *
 * 非-OK 响应抛 `ApiError`（带 code / codeId / localized message）—— 调用方可以
 * `if (err.codeId === '3005')` 安全 dispatch，**不要** substring 匹配 message
 * （i18n 后 message 是 localized 文本，会因 locale 变化）。
 */
export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${API_BASE}${path}`, {
    headers: { 'Content-Type': 'application/json', ...init?.headers },
    ...init,
  })
  if (!res.ok) {
    throw await toApiError(res)
  }
  if (init?.method === 'DELETE' || res.status === 204) {
    // 无响应体的接口：直接返回 undefined，避免 res.json() 报错。
    return undefined as T
  }
  const text = await res.text()
  return (text ? JSON.parse(text) : undefined) as T
}

// ─── 搜索（任务轮询） ─────────────────────────────────────────
// POST   /api/search              → { task_id }
// GET    /api/search/{task_id}    → SearchStatusResponse（轮询当前累计）
// DELETE /api/search/{task_id}    → 204（幂等，清理）

export interface StartSearchResult {
  task_id: number
}

export function createSearch(keyword: string, sourceId?: number): Promise<StartSearchResult> {
  const body: Record<string, unknown> = { keyword }
  if (sourceId != null) body.source_id = sourceId
  return apiFetch<StartSearchResult>('/search', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function getSearchStatus(taskId: number): Promise<SearchStatusResponse> {
  return apiFetch<SearchStatusResponse>(`/search/${taskId}`)
}

export function deleteSearch(taskId: number): Promise<void> {
  return apiFetch<void>(`/search/${taskId}`, { method: 'DELETE' })
}

// ─── 书籍详情 + 目录（JSON） ─────────────────────────────────
// GET /api/book/detail?url=&source_id=  → Book
// GET /api/book/toc?url=&source_id=     → { book: Book, chapters: Chapter[] }

export function getBook(bookUrl: string, sourceId: number): Promise<Book> {
  const params = new URLSearchParams({ url: bookUrl, source_id: String(sourceId) })
  return apiFetch<Book>(`/book/detail?${params.toString()}`)
}

export async function getToc(
  bookUrl: string,
  sourceId: number,
): Promise<{ book: Book; chapters: Chapter[] }> {
  const params = new URLSearchParams({ url: bookUrl, source_id: String(sourceId) })
  return apiFetch<{ book: Book; chapters: Chapter[] }>(
    `/book/toc?${params.toString()}`,
  )
}

// ─── 下载（JSON task_id，进度走任务轮询） ─────────────────────
// POST /api/download { url, source_id, ... } → { task_id }
// 进度由 GET /api/tasks 轮询（useTasks 已有 refetchInterval 机制），无 SSE。

export interface DownloadOptions {
  url: string
  sourceId: number
  format?: ExportFormat
  chapterStart?: number
  chapterEnd?: number
}

export async function startDownload(opts: DownloadOptions): Promise<StartDownloadResult> {
  const body: Record<string, unknown> = { url: opts.url, source_id: opts.sourceId }
  if (opts.format) body.format = opts.format
  if (opts.chapterStart != null) body.chapter_start = opts.chapterStart
  if (opts.chapterEnd != null) body.chapter_end = opts.chapterEnd
  return apiFetch<StartDownloadResult>('/download', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

// ─── 任务管理（JSON） ─────────────────────────────────────────
// GET    /api/tasks            → Task[]
// POST   /api/tasks/:id/cancel → "已取消"
// DELETE /api/tasks/:id        → "已删除任务" —— 删任务**记录**，磁盘文件保留。

export function getTasks(): Promise<Task[]> {
  return apiFetch<Task[]>('/tasks')
}

export function cancelTask(id: number): Promise<void> {
  return apiFetch<void>(`/tasks/${id}/cancel`, { method: 'POST' })
}

/** 从 tasks.json 删除一条任务记录（不动磁盘）。成功后 ['tasks'] 失效触发刷新。 */
export function deleteTask(id: number): Promise<void> {
  return apiFetch<void>(`/tasks/${id}`, { method: 'DELETE' })
}

// ─── 书库（JSON） ─────────────────────────────────────────────
// GET    /api/library            → LibraryFile[]
// DELETE /api/library/:filename  → "已删除"

export function getLibrary(): Promise<LibraryFile[]> {
  // 后端暂不支持 ?ext= 服务端过滤（待 Task 10 加）；前端按 ext 客户端过滤即可。
  return apiFetch<LibraryFile[]>('/library')
}

export function deleteFile(filename: string): Promise<void> {
  return apiFetch<void>(`/library/${encodeURIComponent(filename)}`, {
    method: 'DELETE',
  })
}

/** 下载书库文件（触发浏览器下载）。文件名经后端 sanitize_filename 处理。 */
export function fileDownloadUrl(filename: string): string {
  return `${API_BASE}/files/${encodeURIComponent(filename)}`
}

// ─── 书源（JSON） ─────────────────────────────────────────────
// GET  /api/sources            → Source[]
// POST /api/sources/:id/toggle → Source（无 body，切换禁用状态）
// POST /api/sources/:id/test   → SourceTestResult

export function getSources(): Promise<Source[]> {
  return apiFetch<Source[]>('/sources')
}

export function toggleSource(id: number): Promise<Source> {
  // 后端 toggle 是无 body 的切换，返回更新后的 SourceInfo。前端 enabled 状态从返回值取。
  return apiFetch<Source>(`/sources/${id}/toggle`, { method: 'POST' })
}

export function testSource(id: number): Promise<SourceTestResult> {
  return apiFetch<SourceTestResult>(`/sources/${id}/test`, { method: 'POST' })
}

// ─── 设置（JSON） ─────────────────────────────────────────────
// GET /api/settings → AppConfig（完整）
// PUT /api/settings → AppConfig（更新后）；body 为部分字段（SettingsUpdate）

export function getSettings(): Promise<Settings> {
  return apiFetch<Settings>('/settings')
}

export function saveSettings(settings: Partial<Settings>): Promise<Settings> {
  return apiFetch<Settings>('/settings', {
    method: 'PUT',
    body: JSON.stringify(settings),
  })
}

