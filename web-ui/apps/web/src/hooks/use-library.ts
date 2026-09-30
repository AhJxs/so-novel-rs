// 书库 hooks —— 列表查询 + 文件删除。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { deleteFile, getLibrary } from '@/lib/api'

/** 书库文件列表。 */
export function useLibrary() {
  return useQuery({
    queryKey: ['library'],
    queryFn: getLibrary,
  })
}

/** 删除书库文件（删磁盘），成功后失效 ['library']。 */
export function useDeleteFile() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (filename: string) => deleteFile(filename),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['library'] }),
  })
}
