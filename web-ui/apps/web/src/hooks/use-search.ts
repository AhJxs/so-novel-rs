// useSearch：封装 SearchContext 的访问。单独拆出是为了让 search-context.tsx 只导出组件，
// 满足 react-refresh / fast refresh 要求。

import { useContext } from 'react'
import { SearchContext, type UseSearchReturn } from '@/contexts/search-context'

export function useSearch(): UseSearchReturn {
  const ctx = useContext(SearchContext)
  if (!ctx) {
    throw new Error('useSearch must be used within <SearchProvider>')
  }
  return ctx
}