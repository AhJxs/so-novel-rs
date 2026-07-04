// 搜索状态 Context —— 把 useSearch 的状态提到应用根，跨路由切换保留结果。
// 之前状态在 SearchPage 内部 useState，组件卸载即清空；切到其他路由再回来结果就没了。
// 现在用一个 Provider 包裹整个 App，状态活在 Provider 树里，路由切换只换页面不卸载 Provider。
// useSearch hook 单独放 hooks/use-search.ts，满足 react-refresh "一个文件只导出组件" 的要求。

import { createContext, useCallback, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { searchBooks } from '@/lib/api'
import type { SearchResult } from '@/lib/types'

export interface UseSearchReturn {
  /** 已累加的搜索结果（所有源合并）。 */
  results: SearchResult[]
  /** 是否正在拉取流（searchBooks 的 promise 未结束）。 */
  isFetching: boolean
  /** 是否已发起过至少一次搜索（用于区分初始空态与未搜索态）。 */
  searched: boolean
  /** 已返回结果的源数量（每收到一个 result 事件 +1，含出错源）。 */
  sourceCount: number
  /** 流级或源级错误信息（取首个非空）。 */
  error: string | null
  /** 发起搜索；若上次未完成会先 abort。可选外部 signal 联动取消。 */
  search: (keyword: string, sourceId?: number, signal?: AbortSignal) => Promise<void>
  /** 重置全部状态并取消进行中的搜索。 */
  reset: () => void
}

// eslint-disable-next-line react-refresh/only-export-components
export const SearchContext = createContext<UseSearchReturn | null>(null)

export function SearchProvider({ children }: { children: ReactNode }) {
  const [results, setResults] = useState<SearchResult[]>([])
  const [isFetching, setFetching] = useState(false)
  const [searched, setSearched] = useState(false)
  const [sourceCount, setSourceCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  const reset = useCallback(() => {
    if (abortRef.current) {
      abortRef.current.abort()
      abortRef.current = null
    }
    setResults([])
    setFetching(false)
    setSearched(false)
    setSourceCount(0)
    setError(null)
  }, [])

  const search = useCallback(
    async (keyword: string, sourceId?: number, signal?: AbortSignal) => {
      // 取消上次未完成的搜索
      if (abortRef.current) {
        abortRef.current.abort()
      }
      const controller = new AbortController()
      abortRef.current = controller
      // 联动调用方传入的外部 signal
      if (signal) {
        if (signal.aborted) {
          controller.abort()
        } else {
          signal.addEventListener('abort', () => controller.abort(), { once: true })
        }
      }

      setResults([])
      setError(null)
      setSourceCount(0)
      setSearched(true)
      setFetching(true)

      try {
        await searchBooks(
          keyword,
          {
            onResult: (ev) => {
              // 源级错误：取首个非空存入 error（不阻断后续源）
              if (ev.error) {
                setError((prev) => prev ?? ev.error)
              }
              if (ev.results && ev.results.length > 0) {
                setResults((prev) => [...prev, ...ev.results])
              }
              setSourceCount((prev) => prev + 1)
            },
            onError: (err) => {
              // 流级错误（fetch 失败 / 非 2xx）；abort 触发的忽略
              if (!controller.signal.aborted) {
                setError(err.message)
              }
            },
          },
          sourceId,
          controller.signal,
        )
      } catch (e) {
        const err = e instanceof Error ? e : new Error(String(e))
        // abort 不算错误
        if (!controller.signal.aborted) {
          setError(err.message)
        }
      } finally {
        if (abortRef.current === controller) {
          abortRef.current = null
        }
        setFetching(false)
      }
    },
    [],
  )

  return (
    <SearchContext.Provider value={{ results, isFetching, searched, sourceCount, error, search, reset }}>
      {children}
    </SearchContext.Provider>
  )
}