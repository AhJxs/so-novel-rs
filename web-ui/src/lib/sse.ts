// SSE（Server-Sent Events）解析工具。
//
// 后端 `axum::response::Sse` 每条事件形如：
// ```
// event: <name>\n
// data: <json>\n
// \n
// ```
// 不同事件用空行分隔；一个事件可以有多个 `data:` 行，合并时用 `\n` 拼接。
//
// 此模块导出三个纯函数（与 DOM 类型无关）以便 `node --test` 跑单测：
// - `parseSseBlock(raw)`：一段 SSE 块（不含末尾空行）→ `{ event, data }` 或 null
// - `consumeSse(res, onEvent, signal?)`：把一个 `Response.body` 拉到底，逐块解析
// - `SseEvent` 类型

export interface SseEvent {
  /** 事件名（来自 `event:` 行；缺省为 "message"） */
  event: string
  /** data 行的内容（多行用 \n 拼接） */
  data: string
}

/**
 * 解析单条 SSE 块。
 *
 * 输入：一段 SSE 块文本（不含末尾空行 `\n\n`）。
 * 输出：解析后的 `{ event, data }`；若 data 行为空则返回 `null`。
 *
 * 规则：
 * - `event: foo` → event = "foo"（最后一次出现胜出）
 * - `data: bar` → 收集到 dataLines；行首**单个**前导空格（`data: {...}` 的
 *   `: ` 紧贴）会被剥掉，符合 SSE 规范
 * - 其它行（`id:` / `retry:` / 注释 `:foo` / 空）忽略
 */
export function parseSseBlock(raw: string): SseEvent | null {
  let event = 'message'
  const dataLines: string[] = []
  for (const line of raw.split('\n')) {
    if (line.startsWith('event:')) {
      event = line.slice(6).trim()
    } else if (line.startsWith('data:')) {
      // 剥行首一个空格（spec：SSE data 行首的单个空格视为 padding，不计入 value）
      dataLines.push(line.slice(5).replace(/^ /, ''))
    }
  }
  if (dataLines.length === 0) return null
  return { event, data: dataLines.join('\n') }
}

/**
 * 把一个 SSE 响应的 `ReadableStream<Uint8Array>` 拉到结束，逐条事件回调。
 *
 * - `signal` 中断时立即 cancel reader、提前 return。
 * - 内部按 `\n\n` 切块；最后一段可能不完整（流结束时），会丢弃（spec：完整事件必
 *   须以空行结束，不完整块视为丢包）。
 */
export async function consumeSse(
  res: { body: ReadableStream<Uint8Array> | null },
  onEvent: (ev: SseEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  if (!res.body) return
  const reader = res.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  const close = async () => {
    try {
      await reader.cancel()
    } catch {
      /* ignore */
    }
  }

  try {
    while (true) {
      if (signal?.aborted) {
        await close()
        return
      }
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })

      // SSE 以空行分隔事件；buffer 末尾可能含不完整块，保留。
      let sep: number
      while ((sep = buffer.indexOf('\n\n')) !== -1) {
        const raw = buffer.slice(0, sep)
        buffer = buffer.slice(sep + 2)
        const ev = parseSseBlock(raw)
        if (ev) onEvent(ev)
      }
    }
  } finally {
    await close()
  }
}
