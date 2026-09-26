// 下载 hook —— POST /api/download 返回 { task_id } 即任务已入 state.tasks。
// 进度不在此追踪（无 SSE）：任务页用 useTasks 轮询 GET /api/tasks 展示。

import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { startDownload } from '@/lib/api'
import type { DownloadOptions } from '@/lib/api'
import type { StartDownloadResult } from '@/lib/types'

export interface UseDownloadReturn {
  /**
   * 启动下载并返回 { task_id }。resolve 时后端已 push 任务到 state.tasks，
   * 并已 invalidate ['tasks']，调用方跳转 /tasks 后列表 refetch 可见新任务。
   */
  start: (opts: DownloadOptions) => Promise<StartDownloadResult>
}

export function useDownload(): UseDownloadReturn {
  const qc = useQueryClient()
  const start = useCallback(
    async (opts: DownloadOptions) => {
      const res = await startDownload(opts)
      qc.invalidateQueries({ queryKey: ['tasks'] })
      return res
    },
    [qc],
  )
  return { start }
}
