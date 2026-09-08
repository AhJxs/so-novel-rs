// 书籍详情/目录 hooks —— 从 use-search.ts 拆出来。
// useSearch 已迁到 contexts/search-context.tsx（需要 Provider 跨路由保留状态），
// 这两个只是普通 useQuery，不依赖搜索上下文。

import { useQuery } from '@tanstack/react-query'
import { getBook, getToc } from '@/lib/api'

/**
 * 书籍详情。enabled = !!url && sourceId != null，避免空查询触发请求。
 */
export function useBookDetail(url: string | null, sourceId: number | null) {
  return useQuery({
    queryKey: ['book', url, sourceId],
    queryFn: () => getBook(url!, sourceId!),
    enabled: !!url && sourceId != null,
  })
}

/**
 * 书籍目录。enabled = !!url && sourceId != null。
 */
export function useToc(url: string | null, sourceId: number | null) {
  return useQuery({
    queryKey: ['toc', url, sourceId],
    queryFn: () => getToc(url!, sourceId!),
    enabled: !!url && sourceId != null,
  })
}