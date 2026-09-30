// 下载任务页面：轮询 Task 列表 + 进度条 + 取消 / 删除（AlertDialog 二次确认）。
// 进度：total_chapters 已确定 → 定量进度条；解析阶段（=0）→ indeterminate。

import { useState, useEffect, useCallback, useMemo } from "react"
import { ArrowDown, ArrowDownToLine, Ban, CheckCircle2, CircleX } from "lucide-react"
import { Badge } from "@workspace/ui/components/badge"
import { Button } from "@workspace/ui/components/button"
import { Card, CardContent } from "@workspace/ui/components/card"
import { Progress } from "@workspace/ui/components/progress"
import {
  Pagination,
  PaginationContent,
  PaginationEllipsis,
  PaginationItem,
  PaginationLink,
  PaginationNext,
  PaginationPrevious,
} from "@workspace/ui/components/pagination"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@workspace/ui/components/alert-dialog"
import { toast } from "sonner"
import { useTasks, useCancelTask, useDeleteTask } from "@/hooks/use-tasks"
import { fileDownloadUrl } from "@/lib/api"
import { formatUnixDate } from "@/lib/utils"
import { useTranslation } from "react-i18next"
import type { Task } from "@/lib/types"

const PAGE_SIZE = 12

// 状态映射：color 给 icon/badge 文字，bg 给卡片左侧色块。
const STATUS_MAP: Record<string, { labelKey: string; color: string; bg: string }> = {
  Downloading: { labelKey: "tasks.status.downloading", color: "text-blue-500", bg: "bg-blue-100 dark:bg-blue-900" },
  Finished:    { labelKey: "tasks.status.finished", color: "text-green-500", bg: "bg-green-100 dark:bg-green-900" },
  Failed:      { labelKey: "tasks.status.failed", color: "text-red-500", bg: "bg-red-100 dark:bg-red-900" },
  Cancelled:   { labelKey: "tasks.status.cancelled", color: "text-gray-400", bg: "bg-gray-100 dark:bg-gray-800" },
}

// 顶部 badge 背景：状态色 15% 半透明
const STATUS_BADGE_BG: Record<keyof typeof STATUS_MAP, string> = {
  Downloading: "bg-blue-500/15 text-blue-500",
  Finished:    "bg-green-500/15 text-green-500",
  Failed:      "bg-red-500/15 text-red-500",
  Cancelled:   "bg-gray-500/15 text-gray-400",
}

export default function TasksPage() {
  const { data: tasks = [] } = useTasks()
  const { mutate: cancel } = useCancelTask()
  const { mutate: delTask } = useDeleteTask()
  const { t } = useTranslation()
  const [page, setPage] = useState(1)
  const [pending, setPending] = useState<Task | null>(null)

  const totalPages = Math.max(1, Math.ceil(tasks.length / PAGE_SIZE))
  const paged = tasks.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  // 顶部状态 badge：固定顺序，计数 = 0 不渲染
  const statusCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const task of tasks) counts[task.status] = (counts[task.status] ?? 0) + 1
    return counts
  }, [tasks])

  // 任务数变化时把页码夹回合法范围
  useEffect(() => {
    setPage((p) => Math.min(p, totalPages))
  }, [totalPages])

  const confirmDelete = () => {
    if (!pending) return
    delTask(pending.id, {
      onSuccess: () => toast.success(t("tasks.deleted")),
    })
    setPending(null)
  }

  const pageItems = useCallback((): ("ellipsis" | number)[] => {
    if (totalPages <= 7) return Array.from({ length: totalPages }, (_, i) => i + 1)
    const items: ("ellipsis" | number)[] = [1]
    if (page > 3) items.push("ellipsis")
    const start = Math.max(2, page - 1)
    const end = Math.min(totalPages - 1, page + 1)
    for (let i = start; i <= end; i++) items.push(i)
    if (page < totalPages - 2) items.push("ellipsis")
    items.push(totalPages)
    return items
  }, [page, totalPages])

  return (
    <div className="flex flex-col gap-4">
      {tasks.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          {(["Downloading", "Finished", "Failed", "Cancelled"] as const).map((status) => {
            const n = statusCounts[status] ?? 0
            if (n === 0) return null
            return (
              <Badge key={status} className={STATUS_BADGE_BG[status]}>
                {t(STATUS_MAP[status].labelKey)} · {n}
              </Badge>
            )
          })}
        </div>
      )}

      {tasks.length === 0 ? (
        <div className="py-20 text-center text-muted-foreground">
          <ArrowDown className="mx-auto mb-3 size-12 opacity-40" />
          <p>{t("tasks.empty")}</p>
        </div>
      ) : (
        <>
          <div className="flex flex-col gap-3">
            {paged.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                onCancel={() => cancel(task.id)}
                onDelete={() => setPending(task)}
              />
            ))}
          </div>

          {totalPages > 1 && (
            <div className="pt-2">
              <Pagination className="justify-end">
                <PaginationContent>
                  <PaginationItem>
                    <PaginationPrevious
                      text=""
                      aria-disabled={page === 1}
                      onClick={() => setPage((p) => Math.max(1, p - 1))}
                    />
                  </PaginationItem>
                  {pageItems().map((n, i) =>
                    n === "ellipsis" ? (
                      <PaginationItem key={`e-${i}`}>
                        <PaginationEllipsis />
                      </PaginationItem>
                    ) : (
                      <PaginationItem key={n}>
                        <PaginationLink isActive={n === page} onClick={() => setPage(n)}>
                          {n}
                        </PaginationLink>
                      </PaginationItem>
                    )
                  )}
                  <PaginationItem>
                    <PaginationNext
                      text=""
                      aria-disabled={page === totalPages}
                      onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                    />
                  </PaginationItem>
                </PaginationContent>
              </Pagination>
            </div>
          )}
        </>
      )}

      {/* 删除确认 */}
      <AlertDialog open={pending !== null} onOpenChange={(open) => { if (!open) setPending(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("tasks.deleteConfirm.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("tasks.deleteConfirm.message", {
                name: pending?.filename ?? pending?.book_name ?? (pending ? `#${pending.id}` : ""),
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("tasks.deleteConfirm.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {t("tasks.deleteConfirm.confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

function TaskCard({ task: t, onCancel, onDelete }: { task: Task; onCancel: () => void; onDelete: () => void }) {
  const { t: translate } = useTranslation()
  const s = STATUS_MAP[t.status] ?? STATUS_MAP.Downloading
  const pct = t.total_chapters > 0 ? Math.round((t.current_chapter / t.total_chapters) * 100) : 0
  // 已结束的任务（含失败 / 取消）保留最后一次进度
  const showProgress = t.total_chapters > 0
  const isActive = t.status === "Downloading"

  const Icon =
    t.status === "Downloading"
      ? ArrowDown
      : t.status === "Finished"
        ? CheckCircle2
        : t.status === "Failed"
          ? CircleX
          : Ban

  return (
    <Card>
      <CardContent className="flex flex-col gap-3 p-5">
        <div className="flex items-center gap-3">
          <div className={`flex size-10 flex-shrink-0 items-center justify-center rounded-lg ${s.bg}`}>
            <Icon className={`size-5 ${s.color} ${isActive ? "animate-spin" : ""}`} />
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-semibold">{t.book_name ?? `任务 #${t.id}`}</p>
            <Badge variant="secondary" className={`mt-0.5 text-xs ${s.color}`}>
              {translate(s.labelKey)}
            </Badge>
          </div>
          <div className="flex flex-shrink-0 gap-1">
            {isActive && (
              <Button variant="destructive" size="sm" onClick={onCancel}>
                {translate("tasks.cancel")}
              </Button>
            )}
            {(t.status === "Finished" || t.status === "Failed" || t.status === "Cancelled") && (
              <Button variant="destructive" size="sm" onClick={onDelete}>
                {translate("tasks.delete")}
              </Button>
            )}
          </div>
        </div>

        {/* 解析阶段（total_chapters=0 且仍 Downloading）用不定态，否则定量 */}
        {(showProgress || (isActive && t.total_chapters === 0)) && (
          <div className="flex flex-col gap-1.5">
            {showProgress && (
              <div className="flex justify-between text-xs text-muted-foreground">
                <span className="flex min-w-0 items-center gap-3">
                  <span>
                    {translate("tasks.chapters", {
                      current: t.current_chapter,
                      total: t.total_chapters,
                    })}
                  </span>
                  {t.failed > 0 && (
                    <span className="text-destructive">
                      {translate("tasks.failedCount", { n: t.failed })}
                    </span>
                  )}
                </span>
                {isActive && <span>{pct}%</span>}
              </div>
            )}
            <Progress
              value={showProgress ? pct : null}
              aria-label={translate(s.labelKey)}
              className="[&>[data-slot=progress-track]]:h-1.5"
            />
          </div>
        )}

        {t.started_at_unix > 0 && (
          <p className="mt-1 text-xs text-muted-foreground">
            {translate("tasks.started")}: {formatUnixDate(t.started_at_unix)}
            {t.finished_at_unix
              ? ` · ${translate("tasks.finished")}: ${formatUnixDate(t.finished_at_unix)}`
              : ""}
          </p>
        )}

        {t.status === "Finished" && t.filename && (
          <div className="mt-1 flex items-center justify-between gap-2">
            <span className="truncate text-xs text-muted-foreground">{t.filename}</span>
            <Button
              size="sm"
              onClick={() => {
                const a = document.createElement("a")
                a.href = fileDownloadUrl(t.filename!)
                a.download = t.filename!
                a.click()
              }}
            >
              <ArrowDownToLine data-icon="inline-start" />
              {translate("tasks.downloadFile")}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
