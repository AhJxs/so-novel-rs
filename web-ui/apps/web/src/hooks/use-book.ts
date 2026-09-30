// 书籍详情/目录 hooks。useSearch 已迁到 contexts/search-context.tsx（需 Provider 跨路由保留状态），
// 这两个是普通 useQuery，不依赖搜索上下文。

import { useQuery } from '@tanstack/react-query'
import { getBook, getToc } from '@/lib/api'

/** 书籍详情；enabled = !!url && sourceId != null，避免空查询触发请求。 */
export function useBookDetail(url: string | null, sourceId: number | null) {
  return useQuery({
    queryKey: ['book', url, sourceId],
    queryFn: () => getBook(url!, sourceId!),
    enabled: !!url && sourceId != null,
  })
}

/** 书籍目录；enabled 同 useBookDetail。 */
export function useToc(url: string | null, sourceId: number | null) {
  return useQuery({
    queryKey: ['toc', url, sourceId],
    queryFn: () => getToc(url!, sourceId!),
    enabled: !!url && sourceId != null,
  })
}