// 下载 hook —— 消费 POST /api/download 的 SSE 进度流。
//
// start() 调 startDownload 并把 AbortController 存 ref，回返的 `started` promise
// 在后端 fetch 返回 + 状态 OK 时 resolve —— 此时后端已经 push 任务到 state.tasks，
// 调用方可在 navigate('/tasks') 之前 await 它，避免 tasks 页拿到旧缓存 + 没就位
// 的 refetch / `refetchInterval` 不轮询，导致新任务不显示。
// 进度按 ev.type 更新 state；终结事件失效 ['tasks'] / ['library'] 让列表刷新。

import { useCallback, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { startDownload } from '@/lib/api'
import type { DownloadOptions } from '@/lib/api'
import type { DownloadProgressEvent } from '@/lib/types'

export interface DownloadState {
  /** 后端分配的任务 id（finished 事件或 taskId promise 给出）。 */
  taskId: number | null
  /** 总章节数（book_resolved 时设定）。 */
  total: number
  /** 已处理章节数（chapter_done / chapter_failed 累加）。 */
  current: number
  /** 流是否已终结（finished/cancelled/failed/error）。 */
  finished: boolean
  /** 失败原因（failed 事件或 error）。 */
  error: string | null
}

const initialState: DownloadState = {
  taskId: null,
  total: 0,
  current: 0,
  finished: false,
  error: null,
}

export interface UseDownloadReturn {
  state: DownloadState
  /**
   * 启动下载并返回 `started` promise：resolve 时表示后端已 push 任务到 state.tasks。
   * 调用方负责在跳转 /tasks 之前 `await start(...)`，避免 race：后端的 POST 还
   * 没到、跳转已经发生、`useTasks()` 拿到旧的缓存（无 Downloading），
   * `refetchInterval` 关掉了，就看不见新任务。
   */
  start: (opts: DownloadOptions) => Promise<void>
  cancel: () => void
}

/**
 * 下载进度状态机。
 * - start：建 AbortController 存 ref，reset state，调 startDownload，返回 started。
 * - onStarted：后端 fetch 返回 + OK，invalidate ['tasks'] 让跳转后任务列表 refetch。
 * - onProgress：按 ev.type 分支更新；终结事件 invalidate tasks/library。
 * - taskId promise resolve 后补设 state.taskId（兼容后端提前给 id 的情况）。
 * - cancel：abort controller + 乐观置 finished 防卡。
 */
export function useDownload(): UseDownloadReturn {
  const qc = useQueryClient()
  const [state, setState] = useState<DownloadState>(initialState)
  const abortRef = useRef<AbortController | null>(null)

  const start = useCallback(
    (opts: DownloadOptions): Promise<void> => {
      // 取消上次未完成的下载
      if (abortRef.current) {
        abortRef.current.abort()
      }
      const controller = new AbortController()
      abortRef.current = controller

      setState({ ...initialState })

      const handle = startDownload(
        opts,
        {
          onStarted: () => {
            // 后端已 push 任务到 state.tasks。让 tasks 列表下次 mount 时
            // refetch 拿到它；强制 invalidate 避免 staleTime 命中缓存。
            qc.invalidateQueries({ queryKey: ['tasks'] })
          },
          onProgress: (ev: DownloadProgressEvent) => {
            setState((prev) => {
              switch (ev.type) {
                case 'book_resolved':
                  // 设定 total（若后端给出）；记录 task_id
                  return {
                    ...prev,
                    total: ev.total ?? prev.total,
                    taskId: ev.task_id ?? prev.taskId,
                  }
                case 'chapter_done':
                case 'chapter_failed':
                  return {
                    ...prev,
                    current: prev.current + 1,
                    taskId: ev.task_id ?? prev.taskId,
                  }
                case 'finished':
                  qc.invalidateQueries({ queryKey: ['tasks'] })
                  qc.invalidateQueries({ queryKey: ['library'] })
                  return {
                    ...prev,
                    finished: true,
                    taskId: ev.task_id ?? prev.taskId,
                    current: ev.total ?? prev.total ?? prev.current,
                  }
                case 'cancelled':
                  qc.invalidateQueries({ queryKey: ['tasks'] })
                  return {
                    ...prev,
                    finished: true,
                    taskId: ev.task_id ?? prev.taskId,
                  }
                case 'failed':
                  qc.invalidateQueries({ queryKey: ['tasks'] })
                  return {
                    ...prev,
                    finished: true,
                    error: ev.reason ?? '下载失败',
                    taskId: ev.task_id ?? prev.taskId,
                  }
                default:
                  return prev
              }
            })
          },
          onError: (err) => {
            // abort（用户取消）触发的忽略；其余记为失败
            if (controller.signal.aborted) return
            setState((prev) => ({ ...prev, finished: true, error: err.message }))
          },
        },
        controller.signal,
      )

      // taskId promise resolve 后补设（用于 finished 之前后端提前给 id 的情况；
      // api.ts 当前只在终结事件 resolve，故多数情况为 no-op）
      handle.taskId.then((id) => {
        if (id != null) {
          setState((prev) => (prev.taskId == null ? { ...prev, taskId: id } : prev))
        }
      })

      // 错误兜底：consumeSseStream 在 fetch 阶段被 abort 会 reject，此处吸收
      handle.done.catch((e) => {
        if (controller.signal.aborted) return
        const err = e instanceof Error ? e : new Error(String(e))
        setState((prev) => ({ ...prev, finished: true, error: prev.error ?? err.message }))
      })

      // 返回 started，让调用方控制跳转时机
      return handle.started
    },
    [qc],
  )

  const cancel = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort()
      // 乐观置 finished 防止 UI 卡住；流会自然结束
      setState((prev) => ({ ...prev, finished: true }))
    }
  }, [])

  return { state, start, cancel }
}
