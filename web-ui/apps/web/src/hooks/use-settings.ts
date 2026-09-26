// 设置 hooks —— 读取 + 部分字段更新。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { getSettings, saveSettings } from '@/lib/api'
import type { Settings } from '@/lib/types'

/**
 * 读取完整设置。
 */
export function useSettings() {
  return useQuery({
    queryKey: ['settings'],
    queryFn: getSettings,
  })
}

/**
 * 部分字段更新设置（PUT /api/settings）。成功后失效 ['settings'] 刷新。
 */
export function useSaveSettings() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: Partial<Settings>) => saveSettings(patch),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['settings'] }),
  })
}
