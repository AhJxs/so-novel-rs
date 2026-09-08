// 书源 hooks —— 列表 + 启停切换 + 测速。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getSources, testSource, toggleSource } from '@/lib/api'

/**
 * 书源列表。
 */
export function useSources() {
  return useQuery({
    queryKey: ['sources'],
    queryFn: getSources,
  })
}

/**
 * 切换书源启停（POST 无 body，返回更新后的 Source）。成功后失效 ['sources']。
 */
export function useToggleSource() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: number) => toggleSource(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['sources'] }),
  })
}

/**
 * 书源测速。返回 { ok, latency_ms, error }，无需 invalidate。
 */
export function useTestSource() {
  return useMutation({
    mutationFn: (id: number) => testSource(id),
  })
}
