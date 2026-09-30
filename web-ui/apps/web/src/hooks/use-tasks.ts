// 任务 hooks —— useQuery 轮询 + useMutation 取消 / 删除。
//
// 存在 Downloading 任务时每 1s 轮询 GET /api/tasks，否则停止（refetchInterval 返回 false）；
// 1s 足以在典型章节抓取（200-500ms）中看到 5-20 个中间态。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { cancelTask, deleteTask, getTasks } from '@/lib/api'

/** 任务列表：有任意 Downloading 任务时 1000ms 轮询，否则停止。 */
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

/** 取消任务。成功后失效 ['tasks'] 触发刷新。 */
export function useCancelTask() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => cancelTask(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tasks'] }),
  })
}

/** 删除任务**记录**（不动磁盘，见 `src/web/handlers/download.rs::task_delete`），
 *  与 useLibrary 的 useDeleteFile 语义相反。 */
export function useDeleteTask() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => deleteTask(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['tasks'] }),
  })
}
