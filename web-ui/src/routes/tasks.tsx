// 下载任务页面。轮询 Task 列表，显示进度条，支持取消。
// 进度：total_chapters 已确定 → 定量进度条；解析阶段（=0）→ indeterminate。
// 分页：跟 library/search 同款 HeroUI Pagination（含折叠）。
// 删除走 `<ConfirmDialog>` 二次确认（跟 library 页同模式：pending state 受控）。
// 顶部状态统计：用 4 个不同颜色的 chip（复用 STATUS_MAP）显示各状态任务数，
// 替代之前的「共 N 个任务」文本 —— 状态分布一眼可见，比总数更信息密集。

import { useState, useEffect, useCallback, useMemo } from 'react'
import { ArrowDown, ArrowDownToLine, CircleCheck, CircleXmark, Ban, TrashBin } from '@gravity-ui/icons'
import { Card, Chip, Button, ProgressBar, Pagination } from '@heroui/react'
import { toast } from 'sonner'
import { useTasks, useCancelTask, useDeleteTask } from '@/hooks/use-tasks'
import { fileDownloadUrl } from '@/lib/api'
import ConfirmDialog from '@/components/confirm-dialog'
import { formatUnixDate } from '@/lib/utils'
import { useTranslation } from 'react-i18next'
import type { Task } from '@/lib/types'

const PAGE_SIZE = 12

// 状态映射：color 给 icon/chip 文字用，bg 给卡片左侧色块用。
// 排序在页头 chip 上保持 Downloading → Finished → Failed → Cancelled（用户最关心的「进行中」在最左）。
const STATUS_MAP: Record<string, { labelKey: string; color: string; bg: string }> = {
  Downloading: { labelKey: 'tasks.status.downloading', color: 'text-blue-500', bg: 'bg-blue-100 dark:bg-blue-900' },
  Finished:    { labelKey: 'tasks.status.finished', color: 'text-green-500', bg: 'bg-green-100 dark:bg-green-900' },
  Failed:      { labelKey: 'tasks.status.failed', color: 'text-red-500',   bg: 'bg-red-100 dark:bg-red-900' },
  Cancelled:   { labelKey: 'tasks.status.cancelled', color: 'text-gray-400',  bg: 'bg-gray-100 dark:bg-gray-800' },
}

// 顶部 chip 用：`text-` 给文字色，背景用 `color/15` 半透明（让 soft 形态仍能透出
// 一点颜色深度 —— 比纯 `variant="soft"` 默认的中性灰更"对得上" status 配色）。
const STATUS_CHIP_BG: Record<keyof typeof STATUS_MAP, string> = {
  Downloading: 'bg-blue-500/15',
  Finished:    'bg-green-500/15',
  Failed:      'bg-red-500/15',
  Cancelled:   'bg-gray-500/15',
}

export default function TasksPage() {
  const { data: tasks = [] } = useTasks()
  const { mutate: cancel } = useCancelTask()
  const { mutate: delTask } = useDeleteTask()
  const { t } = useTranslation()
  const [page, setPage] = useState(1)
  // 删除走二次确认（参考 library 页：pending state + ConfirmDialog 受控）。
  // 点击 delete → setPending(task) → dialog open；取消 / 关闭 → setPending(null)；
  // 确认 → 调 delTask(task.id)（DELETE /api/tasks/:id，删记录不动磁盘文件）。
  const [pending, setPending] = useState<Task | null>(null)

  const totalPages = Math.max(1, Math.ceil(tasks.length / PAGE_SIZE))
  const paged = tasks.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  // 顶部 4 个状态 chip：按固定顺序展示（Downloading → Finished → Failed → Cancelled），
  // 计数 = 0 的状态不渲染 —— cancelled/failed=0 时不出灰色 chip 减少视觉噪音。
  // Object.keys 顺序由插入顺序保证（V8 / 主流 JS 引擎），跟 STATUS_MAP 字面量一致。
  const statusCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const t of tasks) counts[t.status] = (counts[t.status] ?? 0) + 1
    return counts
  }, [tasks])

  // 任务数变化时把页码夹回合法范围
  useEffect(() => {
    setPage(p => Math.min(p, totalPages))
  }, [totalPages])

  const confirmDelete = () => {
    if (!pending) return
    // 删任务**记录**（DELETE /api/tasks/:id），磁盘上的下载文件不动。
    // 文件想删时去 /library 页面走 DELETE /api/library/:filename。
    delTask(pending.id, {
      onSuccess: () => toast.success(t('tasks.deleted')),
    })
    setPending(null)
  }

  // 总页数多时折叠中间页：始终保留首页 / 末页 + 当前页前后各 1 页
  const pageItems = useCallback((): ('ellipsis' | number)[] => {
    if (totalPages <= 7) return Array.from({ length: totalPages }, (_, i) => i + 1)
    const items: ('ellipsis' | number)[] = [1]
    if (page > 3) items.push('ellipsis')
    const start = Math.max(2, page - 1)
    const end = Math.min(totalPages - 1, page + 1)
    for (let i = start; i <= end; i++) items.push(i)
    if (page < totalPages - 2) items.push('ellipsis')
    items.push(totalPages)
    return items
  }, [page, totalPages])

  return (
    <div className="space-y-4">
      {tasks.length > 0 && (
        <div className="flex items-center justify-start gap-2 flex-wrap">
          {(['Downloading', 'Finished', 'Failed', 'Cancelled'] as const).map((status) => {
            const n = statusCounts[status] ?? 0
            if (n === 0) return null
            const s = STATUS_MAP[status]
            return (
              <Chip
                key={status}
                size="md"
                variant="soft"
                className={`${s.color} ${STATUS_CHIP_BG[status]}`}
              >
                {t(s.labelKey)} · {n}
              </Chip>
            )
          })}
        </div>
      )}

      {tasks.length === 0 ? (
        <div className="text-center py-20 text-default-500">
          <ArrowDown className="w-12 h-12 mx-auto mb-3 opacity-40" />
          <p>{t('tasks.empty')}</p>
        </div>
      ) : (
        <>
          <div className="space-y-3">
            {paged.map(t => (
              <TaskCard key={t.id} task={t} onCancel={() => cancel(t.id)}
                onDelete={() => setPending(t)} />
            ))}
          </div>

          {/* 分页 */}
          {totalPages > 1 && (
            <div className="pt-2">
              <Pagination className="justify-end">
                <Pagination.Content>
                  <Pagination.Item>
                    <Pagination.Previous
                      isDisabled={page === 1}
                      onPress={() => setPage(p => Math.max(1, p - 1))}
                    >
                      <Pagination.PreviousIcon />
                    </Pagination.Previous>
                  </Pagination.Item>
                  {pageItems().map((n, i) => (
                    n === 'ellipsis' ? (
                      <Pagination.Item key={`e-${i}`}>
                        <Pagination.Ellipsis />
                      </Pagination.Item>
                    ) : (
                      <Pagination.Item key={n}>
                        <Pagination.Link isActive={n === page} onPress={() => setPage(n)}>
                          {n}
                        </Pagination.Link>
                      </Pagination.Item>
                    )
                  ))}
                  <Pagination.Item>
                    <Pagination.Next
                      isDisabled={page === totalPages}
                      onPress={() => setPage(p => Math.min(totalPages, p + 1))}
                    >
                      <Pagination.NextIcon />
                    </Pagination.Next>
                  </Pagination.Item>
                </Pagination.Content>
              </Pagination>
            </div>
          )}
        </>
      )}

      {/* 删除确认：filename 优先（删除的真正内容），缺省用书名 / 任务 #id —— 跟
          library 页的 `pending` state 受控模式同构。filename 缺失的卡（多数
          failed / cancelled 没产生文件）confirm 只收回对话框，不打后端。 */}
      <ConfirmDialog
        isOpen={pending !== null}
        onOpenChange={(open) => { if (!open) setPending(null) }}
        title={t('tasks.deleteConfirm.title')}
        message={t('tasks.deleteConfirm.message', {
          name: pending?.filename ?? pending?.book_name ?? (pending ? `#${pending.id}` : ''),
        })}
        confirmLabel={t('tasks.deleteConfirm.confirm')}
        cancelLabel={t('tasks.deleteConfirm.cancel')}
        onConfirm={confirmDelete}
      />
    </div>
  )
}

function TaskCard({ task: t, onCancel, onDelete }: { task: Task; onCancel: () => void; onDelete: () => void }) {
  const { t: translate } = useTranslation()
  const s = STATUS_MAP[t.status] ?? STATUS_MAP.Downloading
  const pct = t.total_chapters > 0 ? Math.round(t.current_chapter / t.total_chapters * 100) : 0
  // 已结束的任务（包括失败 / 取消）保留最后一次的进度 —— 跟 GPUI row.rs 把 100% 进度条保留显示的逻辑一致，
  // 用户直观知道「跑到一半挂了」而不是「0% 直接失败」。
  const showProgress = t.total_chapters > 0
  const isActive = t.status === 'Downloading'

  const Icon = t.status === 'Downloading' ? ArrowDown : t.status === 'Finished' ? CircleCheck : t.status === 'Failed' ? CircleXmark : Ban

  return (
    <Card className="p-5">
      <div className="flex items-center gap-3 mb-3">
        <div className={`w-10 h-10 rounded-lg ${s.bg} flex items-center justify-center flex-shrink-0`}>
          <Icon className={`w-5 h-5 ${s.color} ${isActive ? 'animate-spin' : ''}`} />
        </div>
        <div className="flex-1 min-w-0">
          <p className="font-semibold text-sm truncate">{t.book_name ?? `任务 #${t.id}`}</p>
          <Chip size="sm" variant="soft" className={`text-xs mt-0.5 ${s.color}`}>{translate(s.labelKey)}</Chip>
        </div>
        <div className="flex gap-1 flex-shrink-0">
          {isActive && (
            <Button variant="danger" size="sm" onPress={onCancel}>
              {translate('tasks.cancel')}
            </Button>
          )}
          {(t.status === 'Finished' || t.status === 'Failed' || t.status === 'Cancelled') && (
            <Button variant="danger" size="sm" onPress={onDelete}>
              <TrashBin/>
              {translate('tasks.delete')}
            </Button>
          )}
        </div>
      </div>

      {/* 进度条：解析阶段（total_chapters=0 且还在 Downloading）用 indeterminate；
          已知总数用定量。已结束的任务（如有 total_chapters）仍展示最后进度 —— 跟 GPUI
          `row.rs` 把 100% / 当前百分比都保留显示一致。

          HeroUI v3 ProgressBar 是 compound API：root 只持有 value/state context，
          真正的 `<track>` / `<fill>` 必须显式放 children 里 —— 没 Track/Fill 就
          什么都不画。`size="sm"` 控制轨道高度（h-1），`color` 用 status 映射；
          indeterminate 时**不传 value**，否则 react-aria 会塞 `aria-valuenow="0"`，
          CSS `:not([aria-valuenow])` 不匹配，indeterminate 动画就不动了。 */}
      {(showProgress || (isActive && t.total_chapters === 0)) && (
        <div className="space-y-1.5">
          {showProgress && (
            <div className="flex justify-between text-xs text-default-500">
              <span className="flex items-center gap-3 min-w-0">
                <span>{translate('tasks.chapters', { current: t.current_chapter, total: t.total_chapters })}</span>
                {t.failed > 0 && (
                  <span className="text-danger">
                    {translate('tasks.failedCount', { n: t.failed })}
                  </span>
                )}
              </span>
              {isActive && <span>{pct}%</span>}
            </div>
          )}
          <ProgressBar
            {...(showProgress ? { value: pct } : {})}
            isIndeterminate={isActive && t.total_chapters === 0}
            size="sm"
            color={
              t.status === 'Finished' ? 'success'
                : t.status === 'Failed' ? 'danger'
                : t.status === 'Cancelled' ? 'default'
                : 'accent'
            }
            aria-label={translate(s.labelKey)}
            className="w-full"
          >
            <ProgressBar.Track>
              <ProgressBar.Fill />
            </ProgressBar.Track>
          </ProgressBar>
        </div>
      )}

      {t.started_at_unix > 0 && (
        <p className="text-xs text-default-500 mt-1.5">
          {translate('tasks.started')}: {formatUnixDate(t.started_at_unix)}
          {t.finished_at_unix ? ` · ${translate('tasks.finished')}: ${formatUnixDate(t.finished_at_unix)}` : ''}
        </p>
      )}

      {t.status === 'Finished' && t.filename && (
        <div className="flex items-center justify-between mt-1.5 gap-2">
          <span className="text-xs text-default-500 truncate">{t.filename}</span>
          {/* HeroUI Button 替代文字链：variant=primary 表示这是 Finished 卡片里的
              主操作；用 `<a download>` 的"click + download 属性"模式触发浏览器
              下载 —— `fileDownloadUrl` 路径由后端 sanitize_filename 编码。 */}
          <Button
            size="sm"
            variant="primary"
            onPress={() => {
              const a = document.createElement('a')
              a.href = fileDownloadUrl(t.filename!)
              a.download = t.filename!
              a.click()
            }}
          >
            <ArrowDownToLine />
            {translate('tasks.downloadFile')}
          </Button>
        </div>
      )}
    </Card>
  )
}