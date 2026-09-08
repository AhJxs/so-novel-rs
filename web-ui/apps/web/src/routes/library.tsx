// 书库页面。按扩展名过滤（前端过滤，后端暂不支持 ?ext=）。
// 提供下载链接（GET /api/files/:filename）+ 删除（带确认对话框）+ 分页。
// 过滤 Tab 右侧挂 Badge 显示每种类型文件数。

import { Book, ArrowDown, Trash2 } from "lucide-react"
import { Badge } from "@workspace/ui/components/badge"
import { Button } from "@workspace/ui/components/button"
import { Card, CardContent } from "@workspace/ui/components/card"
import { Skeleton } from "@workspace/ui/components/skeleton"
import { Tabs, TabsList, TabsTrigger } from "@workspace/ui/components/tabs"
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
import { useState, useEffect, useCallback, useMemo } from "react"
import { useLibrary, useDeleteFile } from "@/hooks/use-library"
import { formatBytes, formatUnixDate } from "@/lib/utils"
import { useTranslation } from "react-i18next"
import type { LibraryFile } from "@/lib/types"

const EXT_COLOR: Record<string, string> = {
  epub: "bg-green-500",
  pdf: "bg-red-500",
  txt: "bg-blue-500",
  html: "bg-orange-500",
  md: "bg-purple-500",
}

const PAGE_SIZE = 12

export default function LibraryPage() {
  const [ext, setExt] = useState<string>("all")
  const [page, setPage] = useState(1)
  const [pending, setPending] = useState<LibraryFile | null>(null) // 待删除文件（打开确认框）
  const { data: allFiles = [], isLoading } = useLibrary()
  const { mutate: del } = useDeleteFile()
  const { t } = useTranslation()

  const files = ext === "all" ? allFiles : allFiles.filter((f) => f.ext === ext)
  const totalPages = Math.max(1, Math.ceil(files.length / PAGE_SIZE))
  const paged = files.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  // 每种 ext 的文件数（含 0）—— 一次 reduce 算 6 个数（O(n)）。
  const extCounts = useMemo(() => {
    const counts: Record<string, number> = { all: allFiles.length, epub: 0, txt: 0, pdf: 0, html: 0, md: 0 }
    for (const f of allFiles) {
      if (f.ext in counts) counts[f.ext]++
    }
    return counts
  }, [allFiles])

  // 切换过滤或文件数变化时，把页码夹回合法范围。
  useEffect(() => {
    setPage((p) => Math.min(p, totalPages))
  }, [totalPages])

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

  const handleFilterChange = (key: string) => {
    setExt(key)
    setPage(1)
  }

  const confirmDelete = () => {
    if (!pending) return
    del(pending.filename, {
      onSuccess: () => toast.success(t("library.deleted")),
    })
    setPending(null)
  }

  return (
    <div className="flex flex-col gap-4">
      <Tabs value={ext} onValueChange={handleFilterChange}>
        <TabsList>
          {["all", "epub", "txt", "pdf", "html", "md"].map((tab) => (
            <TabsTrigger key={tab} value={tab}>
              {t(`library.filter.${tab}`).toUpperCase()}
              <Badge variant="secondary" className="ml-1">
                {extCounts[tab] ?? 0}
              </Badge>
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      {isLoading && (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-16 w-full" />
          ))}
        </div>
      )}

      {!isLoading && files.length === 0 && (
        <div className="py-16 text-center text-muted-foreground">
          <Book className="mx-auto mb-3 size-12 opacity-40" />
          <p>{t("library.empty")}</p>
        </div>
      )}

      <div className="flex flex-col gap-2">
        {paged.map((f) => (
          <Card key={f.filename} className="group">
            <CardContent className="flex items-center gap-4 p-4">
              <div
                className={`flex size-10 flex-shrink-0 items-center justify-center rounded-lg text-xs font-bold text-white ${EXT_COLOR[f.ext] ?? "bg-muted"}`}
              >
                {f.ext.toUpperCase()}
              </div>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{f.filename}</p>
                <p className="text-xs text-muted-foreground">
                  {formatBytes(f.size)} · {formatUnixDate(f.modified)}
                </p>
              </div>
              <div className="flex gap-2">
                <Button variant="outline" size="sm" render={<a href={`/api/files/${encodeURIComponent(f.filename)}`} download />}>
                  <ArrowDown data-icon="inline-start" />
                  {t("library.download")}
                </Button>
                <Button variant="destructive" size="sm" onClick={() => setPending(f)}>
                  <Trash2 data-icon="inline-start" />
                  {t("library.delete")}
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* 分页 */}
      {!isLoading && totalPages > 1 && (
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

      {/* 删除确认 */}
      <AlertDialog open={pending !== null} onOpenChange={(open) => { if (!open) setPending(null) }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("library.deleteConfirm.title")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("library.deleteConfirm.message", { name: pending?.filename ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("library.deleteConfirm.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {t("library.deleteConfirm.confirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
