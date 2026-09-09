// 前端格式化工具。`cn` helper 由 @workspace/ui 提供（packages/ui/lib/utils），
// 此处只保留业务格式化函数。

/** 字节数格式化为人类可读字符串（B / KB / MB / GB）。 */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`
}

/** Unix 秒级时间戳格式化为本地日期时间（中文，月日时分）。 */
export function formatUnixDate(unix: number): string {
  if (!unix) return ''
  return new Date(unix * 1000).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}
