// 任务 hooks —— useQuery 轮询 + useMutation 取消。
//
// GET /api/tasks → Task[]。当存在 status==='Downloading' 的任务时每 1s 轮询，
// 否则停止轮询（refetchInterval 返回 false）。
//
// 1s 而非 2s：与 GPUI 100ms tick 相比 web 是远粗的（走 HTTP 而不是内存 channel），
// 但 1s 仍然比典型章节抓取（200-500ms）高一个量级，progress bar 在中等大小下载里
// 能看到 5-20 个中间态，符合「实时进度」的体感。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { cancelTask, deleteTask, getTasks } from '@/lib/api'

/**
 * 任务列表。有任意 Downloading 任务时 1000ms 轮询，否则停止。
 */
export function useTasks() {
  return useQuery({
    queryKey: ['tasks'],
    queryFn: getTasks,
    refetchInterval: (query) => {
      const tasks = query.state.data
      return tasks?.some((t) => t.status === 'Downloading') ? 1000 : false
    },
  })
}

/**
 * 取消任务。成功后失效 ['tasks'] 触发刷新。
 */
export function useCancelTask() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => cancelTask(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tasks'] }),
  })
}

/**
 * 删除任务**记录**（不动磁盘文件）。成功后失效 ['tasks'] 触发刷新。
 * 跟 useLibrary 里的 `useDeleteFile` 语义不同：那里删磁盘文件，这里删 tasks.json 里的记录。
 * 详见 `src/web/handlers/download.rs::task_delete`。
 */
export function useDeleteTask() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => deleteTask(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tasks'] }),
  })
}
