// `parseSseBlock` / `consumeSse` 单测。
//
// 当前用 `tsc --noEmit` 做静态检查（保证本测试文件能编译通过 + 类型与生产代码一致）。
// 实际运行需要 test runner（node:test + tsx 或 vitest）—— 见 tasks/todo.md P2-10
// "Add Frontend Test Runner Only If It Pays Off" 评估项；SSE 解析逻辑是首个值得
// 引入 runner 的候选（纯函数、无 DOM、~30 行核心算法）。

import { describe, expect, it } from './test-types'
import { parseSseBlock } from './sse'

describe('parseSseBlock', () => {
  it('parses a minimal block with default event name', () => {
    expect(parseSseBlock('data: hello')).toEqual({
      event: 'message',
      data: 'hello',
    })
  })

  it('parses block with explicit event name', () => {
    expect(parseSseBlock('event: progress\ndata: {"i":1}')).toEqual({
      event: 'progress',
      data: '{"i":1}',
    })
  })

  it('strips single leading space from data line (SSE spec)', () => {
    // axum 输出 `data: {...}` 行首一个空格，符合 spec padding 规则
    expect(parseSseBlock('data: {"a":1}')).toEqual({
      event: 'message',
      data: '{"a":1}',
    })
  })

  it('joins multi-line data with newline', () => {
    const block = 'event: log\ndata: line1\ndata: line2'
    expect(parseSseBlock(block)).toEqual({
      event: 'log',
      data: 'line1\nline2',
    })
  })

  it('returns null when no data line present', () => {
    expect(parseSseBlock('event: ping')).toBeNull()
    expect(parseSseBlock('event: ping\nid: 42')).toBeNull()
  })

  it('ignores id / retry / comment lines', () => {
    const block = ': heartbeat\nevent: ping\nretry: 3000\nid: 99\ndata: ok'
    expect(parseSseBlock(block)).toEqual({ event: 'ping', data: 'ok' })
  })

  it('last event: line wins on duplicate', () => {
    const block = 'event: a\ndata: x\nevent: b'
    expect(parseSseBlock(block)).toEqual({ event: 'b', data: 'x' })
  })
})
