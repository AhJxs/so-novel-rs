// API 客户端 —— 所有 fetch 调用集中在此。base URL 固定 /api，不硬编码 host:port。
//
// 后端契约来源：src/web/routes.rs + src/web/handlers/*.rs。
// - 普通 JSON 接口用 apiFetch<T>。
// - 搜索（GET /api/search）与下载（POST /api/download）是 SSE 流，用 fetch + ReadableStream 手动解析，
//   通过 onEvent 回调推送（浏览器原生 EventSource 不支持 POST，且 GET SSE 也需对 event name 分发）。

import type {
  Book,
  Chapter,
  DownloadProgressEvent,
  ExportFormat,
  LibraryFile,
  SearchDoneEvent,
  SearchStreamEvent,
  Settings,
  Source,
  SourceTestResult,
  StartDownloadResult,
  Task,
} from './types'
import { consumeSse, type SseEvent } from './sse'

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

// ─── 搜索（SSE） ──────────────────────────────────────────────

// GET /api/search?keyword=&source_id=&limit=
// event "result" → SearchStreamEvent；event "done" → SearchDoneEvent（流结束）。

export interface SearchCallbacks {
  onResult: (ev: SearchStreamEvent) => void
  onDone?: (ev: SearchDoneEvent) => void
  onError?: (err: Error) => void
}

export function searchBooks(
  keyword: string,
  callbacks: SearchCallbacks,
  sourceId?: number,
  signal?: AbortSignal,
): Promise<void> {
  const params = new URLSearchParams({ keyword })
  if (sourceId != null) params.set('source_id', String(sourceId))
  return consumeSseStream(
    `${API_BASE}/search?${params.toString()}`,
    { method: 'GET' },
    (ev) => {
      if (ev.event === 'result') {
        try {
          callbacks.onResult(JSON.parse(ev.data) as SearchStreamEvent)
        } catch {
          /* skip malformed */
        }
      } else if (ev.event === 'done') {
        try {
          callbacks.onDone?.(JSON.parse(ev.data) as SearchDoneEvent)
        } catch {
          /* skip */
        }
      }
    },
    callbacks.onError,
    signal,
  )
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

// ─── 下载（SSE 流） ───────────────────────────────────────────
// POST /api/download { url, source_id, format?, chapter_start?, chapter_end? }
// 响应为 SSE，event "progress"，data = DownloadProgressEvent。
// task_id 仅在 finished 事件中出现（后端在 finished 时带 task_id）。
// 为兼容 hooks 假设的 startDownload → {task_id} 签名，调用方可在 finished 事件里取 task_id；
// 同时本函数通过 resolveTaskId 返回首个解析到的 task_id，供需要轮询任务的场景使用。

export interface DownloadCallbacks {
  onProgress: (ev: DownloadProgressEvent) => void
  /**
   * 后端 SSE handler 已开始返回流 —— 此时任务已入 `state.tasks`，
   * 调用方可以放心 invalidate ['tasks'] / 跳转到任务页。
   * 比第一条 SSE 事件（`book_resolved`）早：后者还要等 crawler `resolve_book`。
   * 比 fetch 抛错时不会触发（错误走 `onError`）。
   */
  onStarted?: () => void
  /** 任一终结事件（finished/failed/cancelled）触发；返回 true 表示流已结束。 */
  onDone?: (ev: DownloadProgressEvent) => void
  onError?: (err: Error) => void
}

export interface DownloadOptions {
  url: string
  sourceId: number
  format?: ExportFormat
  chapterStart?: number
  chapterEnd?: number
}

/**
 * 起一个下载；返回值三件套：
 * - `done`：整个 SSE 流消费完的 promise（流自然结束 / abort / error）
 * - `taskId`：首个解析到的 task_id（仅终结事件里后端给出）
 * - `started`：`fetch` 返回 + 状态码 OK 时 resolve —— 后端此刻已接受下载、
 *   任务在 `state.tasks` 中。供导航到 /tasks 前等待用，避免列表看不到新任务。
 */
export function startDownload(
  opts: DownloadOptions,
  callbacks: DownloadCallbacks,
  signal?: AbortSignal,
): { done: Promise<void>; taskId: Promise<number | undefined>; started: Promise<void> } {
  let taskIdResolve: (id: number | undefined) => void
  const taskId = new Promise<number | undefined>((resolve) => {
    taskIdResolve = resolve
  })
  let resolved = false

  let startedResolve: () => void
  const started = new Promise<void>((resolve) => {
    startedResolve = resolve
  })
  let startedResolved = false

  const body: Record<string, unknown> = { url: opts.url, source_id: opts.sourceId }
  if (opts.format) body.format = opts.format
  if (opts.chapterStart != null) body.chapter_start = opts.chapterStart
  if (opts.chapterEnd != null) body.chapter_end = opts.chapterEnd

  const done = consumeSseStream(
    `${API_BASE}/download`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    },
    (ev) => {
      if (ev.event !== 'progress') return
      let parsed: DownloadProgressEvent
      try {
        parsed = JSON.parse(ev.data) as DownloadProgressEvent
      } catch {
        return
      }
      callbacks.onProgress(parsed)
      if (
        parsed.type === 'finished' ||
        parsed.type === 'failed' ||
        parsed.type === 'cancelled'
      ) {
        if (!resolved) {
          resolved = true
          taskIdResolve(parsed.task_id)
        }
        callbacks.onDone?.(parsed)
      }
    },
    (err) => {
      if (!resolved) {
        resolved = true
        taskIdResolve(undefined)
      }
      if (!startedResolved) {
        startedResolved = true
        startedResolve()
      }
      callbacks.onError?.(err)
    },
    signal,
    () => {
      if (!startedResolved) {
        startedResolved = true
        startedResolve()
        callbacks.onStarted?.()
      }
    },
  )

  done.finally(() => {
    if (!resolved) {
      resolved = true
      taskIdResolve(undefined)
    }
  })

  return { done, taskId, started }
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

// ─── 共享 SSE 消费实现 ────────────────────────────────────────

async function consumeSseStream(
  url: string,
  init: RequestInit,
  onEvent: (ev: SseEvent) => void,
  onError?: (err: Error) => void,
  signal?: AbortSignal,
  onFetched?: () => void,
): Promise<void> {
  let res: Response
  try {
    res = await fetch(url, { ...init, signal })
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e))
    onError?.(err)
    throw err
  }
  if (!res.ok) {
    // SSE 入口的 HTTP 错误（一般是 4xx 早返）也走 envelope 解析 —— 后端把
    // 锁毒化 / 业务校验失败的错误以 JSON envelope 返（不走 SSE 流）。
    const err = await toApiError(res)
    onError?.(err)
    throw err
  }
  // fetch 已返回 + 状态码 OK —— 后端 SSE handler 已开始返回流（对下载而言，
  // 任务已 push 进 state.tasks）。先于第一条 SSE 事件回调 onFetched，让
  // 调用方在 `book_resolved` 之前就能 navigate / invalidate 任务列表。
  onFetched?.()
  try {
    await consumeSse(res, onEvent, signal)
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e))
    onError?.(err)
    throw err
  }
}

// 保留 StartDownloadResult 类型导出，供 hooks 以 {task_id} 形态桥接（若后续后端改为返回 JSON）。
export type { StartDownloadResult }
