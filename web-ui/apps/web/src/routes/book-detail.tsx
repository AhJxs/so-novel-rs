// 书籍详情页 —— 详情展示 + 目录获取。
// 点击下载直接 start + 跳转到 /tasks，进度在任务页里看（后端任务独立于组件生命周期）。

import { useState } from "react"
import { useParams, useNavigate, useLocation } from "react-router-dom"
import { ChevronLeft } from "lucide-react"
import { Button } from "@workspace/ui/components/button"
import { Badge } from "@workspace/ui/components/badge"
import { Skeleton } from "@workspace/ui/components/skeleton"
import { ScrollArea } from "@workspace/ui/components/scroll-area"
import { ToggleGroup, ToggleGroupItem } from "@workspace/ui/components/toggle-group"
import { Card, CardContent } from "@workspace/ui/components/card"
import { useTranslation } from "react-i18next"
import { useBookDetail, useToc } from "@/hooks/use-book"
import { useDownload } from "@/hooks/use-download"
import type { ExportFormat } from "@/lib/types"

const FORMATS: ExportFormat[] = ["epub", "txt", "html", "pdf", "markdown"]

export default function BookDetailPage() {
  const { bookUrl } = useParams<{ bookUrl: string }>()
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useTranslation()
  const decoded = decodeURIComponent(bookUrl ?? "")
  const sourceId = (location.state as { sourceId?: number } | null)?.sourceId ?? null

  const { data: book, isLoading } = useBookDetail(decoded, sourceId)
  const { data: toc, refetch: loadToc, isFetching: loadingToc } = useToc(decoded, sourceId)
  const chapters = toc?.chapters ?? []
  const { start: startDl } = useDownload()

  const [format, setFormat] = useState<ExportFormat>("epub")

  // 启动下载：等后端把任务 push 到 state.tasks（POST 返回即入库）才跳任务页。
  const handleDownload = async () => {
    if (!bookUrl || sourceId == null) return
    await startDl({ url: decoded, sourceId, format })
    navigate("/tasks")
  }

  const BackButton = (
    <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
      <ChevronLeft data-icon="inline-start" /> {t("book.backToSearch")}
    </Button>
  )

  if (sourceId == null) {
    return (
      <div className="flex flex-col gap-4">
        {BackButton}
        <p className="py-16 text-center text-muted-foreground">{t("book.missingSource")}</p>
      </div>
    )
  }

  if (isLoading) {
    return (
      <div className="flex flex-col gap-4">
        {BackButton}
        <Skeleton className="h-64 w-full" />
      </div>
    )
  }

  if (!book) {
    return (
      <div className="flex flex-col gap-4">
        {BackButton}
        <p className="py-16 text-center text-muted-foreground">{t("book.loadFailed")}</p>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      {BackButton}

      {/* 双栏：左封面 / 右信息+TOC */}
      <div className="grid gap-6 md:grid-cols-[280px_1fr]">
        {/* 封面 */}
        <Card className="self-start">
          <CardContent className="p-4">
            <div className="relative aspect-[3/4] w-full overflow-hidden rounded-lg bg-muted">
              {book.cover_url ? (
                <img
                  src={book.cover_url}
                  alt={book.book_name}
                  referrerPolicy="no-referrer"
                  className="absolute inset-0 h-full w-full object-cover"
                />
              ) : (
                <div className="absolute inset-0 flex items-center justify-center bg-gradient-to-br from-violet-400 to-purple-600 text-3xl font-bold text-white">
                  {book.book_name[0]}
                </div>
              )}
            </div>
          </CardContent>
        </Card>

        {/* 信息 + 目录 + 下载 */}
        <div className="flex min-w-0 flex-col gap-4">
          {/* 基本信息 */}
          <Card>
            <CardContent className="flex flex-col gap-3 p-5">
              <h2 className="text-xl font-bold">{book.book_name}</h2>
              <p className="text-sm text-muted-foreground">{book.author}</p>
              {book.intro && <p className="line-clamp-3 text-sm text-muted-foreground">{book.intro}</p>}
              <div className="flex flex-wrap gap-2">
                {book.status && <Badge variant="secondary">{book.status}</Badge>}
                {book.latest_chapter && (
                  <Badge variant="secondary">{t("book.latestChapter")}: {book.latest_chapter}</Badge>
                )}
              </div>
            </CardContent>
          </Card>

          {/* 目录 */}
          <Card>
            <CardContent className="flex flex-col gap-3 p-5">
              <div className="flex items-center justify-between">
                <h3 className="font-semibold">
                  {t("book.toc")}
                  {chapters.length > 0 && (
                    <span className="ml-1 text-sm text-muted-foreground">
                      ({t("book.chapters", { count: chapters.length })})
                    </span>
                  )}
                </h3>
                <Button variant="ghost" size="sm" onClick={() => loadToc()} disabled={loadingToc}>
                  {loadingToc
                    ? t("book.loading")
                    : chapters.length
                      ? t("book.refreshToc")
                      : t("book.loadToc")}
                </Button>
              </div>
              {chapters.length > 0 && (
                <ScrollArea className="h-56">
                  <div className="grid grid-cols-2 gap-1 pr-4 sm:grid-cols-3">
                    {chapters.slice(0, 200).map((ch) => (
                      <div
                        key={ch.order}
                        className="truncate rounded px-2 py-1 text-xs text-muted-foreground hover:bg-muted"
                      >
                        {ch.order}. {ch.title}
                      </div>
                    ))}
                  </div>
                </ScrollArea>
              )}
              {!loadingToc && chapters.length === 0 && (
                <p className="text-xs text-muted-foreground">{t("book.notLoaded")}</p>
              )}
            </CardContent>
          </Card>

          {/* 下载：选格式 + 启动 */}
          <Card>
            <CardContent className="flex flex-wrap items-center gap-3 bg-muted/40 p-5">
              <span className="text-sm font-medium">{t("book.format")}</span>
              {/* Base UI ToggleGroup 恒为数组 value（multiple 默认 false = 单选） */}
              <ToggleGroup
                value={[format]}
                onValueChange={(v) => {
                  if (v[0]) setFormat(v[0] as ExportFormat)
                }}
              >
                {FORMATS.map((f) => (
                  <ToggleGroupItem key={f} value={f} className="text-xs uppercase">
                    {f}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <Button className="ml-auto" onClick={handleDownload}>
                {t("book.startDownload")}
              </Button>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  )
}
