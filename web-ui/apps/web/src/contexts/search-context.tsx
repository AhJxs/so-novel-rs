// 搜索状态 Context：把 useSearch 的状态提到应用根，跨路由切换保留结果
// （状态在 Provider 内部 useState 会随页面卸载清空，故必须放这里）。
// 数据层：POST /api/search 建任务 → 每 800ms 轮询 GET /api/search/{id}。
// useSearch hook 单独放 hooks/use-search.ts，满足 react-refresh 一个文件只导出组件。

import { createContext, useCallback, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { createSearch, deleteSearch, getSearchStatus } from '@/lib/api'
import type { SearchResult } from '@/lib/types'

export interface UseSearchReturn {
  /** 已累加的搜索结果（所有源合并）。 */
  results: SearchResult[]
  /** 是否正在轮询中（search 的 promise 未结束）。 */
  isFetching: boolean
  /** 是否已发起过至少一次搜索（区分初始空态与未搜索态）。 */
  searched: boolean
  /** 已完成的源数量（含出错源，对应后端 done_sources）。 */
  sourceCount: number
  /** 取首个非空错误。 */
  error: string | null
  /** 发起搜索；自动取消上一轮轮询并清理旧任务。 */
  search: (keyword: string, sourceId?: number) => Promise<void>
  /** 重置全部状态 + 取消轮询 + 清理服务端搜索任务。 */
  reset: () => void
}

// eslint-disable-next-line react-refresh/only-export-components
export const SearchContext = createContext<UseSearchReturn | null>(null)

const POLL_INTERVAL_MS = 800

/** sleep，可被 signal 提前中断（abort 时立即 resolve）。 */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve()
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    function onAbort() {
      clearTimeout(timer)
      resolve()
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

export function SearchProvider({ children }: { children: ReactNode }) {
  const [results, setResults] = useState<SearchResult[]>([])
  const [isFetching, setFetching] = useState(false)
  const [searched, setSearched] = useState(false)
  const [sourceCount, setSourceCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const taskIdRef = useRef<number | null>(null)

  const cleanupSearch = useCallback(() => {
    // 取消轮询 + 服务端清理旧搜索任务（fire-and-forget，服务端 TTL 兜底）
    if (abortRef.current) abortRef.current.abort()
    if (taskIdRef.current != null) {
      void deleteSearch(taskIdRef.current).catch(() => {})
      taskIdRef.current = null
    }
  }, [])

  const reset = useCallback(() => {
    cleanupSearch()
    setResults([])
    setFetching(false)
    setSearched(false)
    setSourceCount(0)
    setError(null)
  }, [cleanupSearch])

  const search = useCallback(
    async (keyword: string, sourceId?: number) => {
      cleanupSearch()
      const controller = new AbortController()
      abortRef.current = controller

      setResults([])
      setError(null)
      setSourceCount(0)
      setSearched(true)
      setFetching(true)

      try {
        const { task_id } = await createSearch(keyword, sourceId)
        taskIdRef.current = task_id
        for (;;) {
          const st = await getSearchStatus(task_id)
          setResults(st.results)
          setSourceCount(st.done_sources)
          const firstErr = st.source_errors[0]
          if (firstErr) setError((prev) => prev ?? firstErr.error)
          if (st.status === 'Done' || controller.signal.aborted) break
          await sleep(POLL_INTERVAL_MS, controller.signal)
        }
      } catch (e) {
        const err = e instanceof Error ? e : new Error(String(e))
        if (!controller.signal.aborted) setError(err.message)
      } finally {
        if (abortRef.current === controller) abortRef.current = null
        setFetching(false)
      }
    },
    [cleanupSearch],
  )

  return (
    <SearchContext.Provider value={{ results, isFetching, searched, sourceCount, error, search, reset }}>
      {children}
    </SearchContext.Provider>
  )
}
