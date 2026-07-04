// useSearch 钩子 —— 封装 SearchContext 的访问。
// 单独拆出来是为了让 contexts/search-context.tsx 只导出组件，
// 满足 react-refresh 的 fast refresh 要求（一个文件只导出组件）。

import { useContext } from 'react'
import { SearchContext, type UseSearchReturn } from '@/contexts/search-context'

export function useSearch(): UseSearchReturn {
  const ctx = useContext(SearchContext)
  if (!ctx) {
    throw new Error('useSearch must be used within <SearchProvider>')
  }
  return ctx
}