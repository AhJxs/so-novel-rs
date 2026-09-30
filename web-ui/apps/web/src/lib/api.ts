// API 客户端：所有 fetch 调用集中在此，base URL 固定 /api。契约来源 src/web/routes.rs + handlers/*.rs。
// 普通 JSON 接口走 apiFetch<T>；搜索与下载是 SSE 流，用 fetch + ReadableStream 手动解析后经 onEvent 推送
// （EventSource 不支持 POST）。

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

/** 后端错误响应（`src/web/error.rs::ErrorEnvelope`）：`{ error: { code, code_id, message } }`。 */
interface BackendErrorBody {
  error: {
    code: string
    code_id: string
    message: string
  }
}

/**
 * API 错误。前端 dispatch 只认 `code` / `codeId` 两个稳定字段，**不要**按 `message` 匹配
 * —— i18n 后它是 localized 文案。
 */
export class ApiError extends Error {
  /** HTTP 状态码 */
  public readonly status: number
  /** WebErrorKind snake_case 短码（错误大类） */
  public readonly code: string
  /** 业务层稳定数字码（dispatch 用，log 用 code） */
  public readonly codeId: string

  constructor(message: string, opts: { status: number; code: string; codeId: string }) {
    super(message)
    this.name = 'ApiError'
    this.status = opts.status
    this.code = opts.code
    this.codeId = opts.codeId
  }
}

/** 从非-OK Response 抽 ApiError；解析失败（非 JSON / 反代截断）降级为 `HTTP {status}`。 */
async function toApiError(res: Response): Promise<ApiError> {
  const text = await res.text().catch(() => res.statusText)
  // 任何一步失败都降级
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
 * JSON fetch 封装：统一拼 /api 前缀，非-OK 抛 ApiError，204 / void 接口返回 undefined。
 * 调用方按 `err.codeId` dispatch，不要 substring 匹配 message。
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
    return undefined as T
  }
  const text = await res.text()
  return (text ? JSON.parse(text) : undefined) as T
}

// 搜索：POST /api/search 建任务 → GET /api/search/{id} 轮询 → DELETE 清理（204 幂等）

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

// 书籍详情 + 目录：GET /api/book/detail 与 /api/book/toc（?url=&source_id=）

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

// 下载：POST /api/download → { task_id }，进度由 GET /api/tasks 轮询，无 SSE。

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

// 任务：GET /api/tasks；POST /api/tasks/:id/cancel；DELETE /api/tasks/:id 只删**记录**，磁盘文件保留。

export function getTasks(): Promise<Task[]> {
  return apiFetch<Task[]>('/tasks')
}

export function cancelTask(id: number): Promise<void> {
  return apiFetch<void>(`/tasks/${id}/cancel`, { method: 'POST' })
}

export function deleteTask(id: number): Promise<void> {
  return apiFetch<void>(`/tasks/${id}`, { method: 'DELETE' })
}

// 书库：GET /api/library；DELETE /api/library/:filename

export function getLibrary(): Promise<LibraryFile[]> {
  // 后端暂无 ?ext= 服务端过滤，前端按 ext 客户端过滤即可。
  return apiFetch<LibraryFile[]>('/library')
}

export function deleteFile(filename: string): Promise<void> {
  return apiFetch<void>(`/library/${encodeURIComponent(filename)}`, {
    method: 'DELETE',
  })
}

/** 下载书库文件（文件名经后端 sanitize_filename 处理）。 */
export function fileDownloadUrl(filename: string): string {
  return `${API_BASE}/files/${encodeURIComponent(filename)}`
}

// 书源：GET /api/sources；POST /api/sources/:id/toggle；POST /api/sources/:id/test

export function getSources(): Promise<Source[]> {
  return apiFetch<Source[]>('/sources')
}

export function toggleSource(id: number): Promise<Source> {
  // 无 body 的切换，enabled 状态从返回的 SourceInfo 取。
  return apiFetch<Source>(`/sources/${id}/toggle`, { method: 'POST' })
}

export function testSource(id: number): Promise<SourceTestResult> {
  return apiFetch<SourceTestResult>(`/sources/${id}/test`, { method: 'POST' })
}

// 设置：GET /api/settings 返回完整 AppConfig；PUT 接受部分字段（SettingsUpdate）。

export function getSettings(): Promise<Settings> {
  return apiFetch<Settings>('/settings')
}

export function saveSettings(settings: Partial<Settings>): Promise<Settings> {
  return apiFetch<Settings>('/settings', {
    method: 'PUT',
    body: JSON.stringify(settings),
  })
}

