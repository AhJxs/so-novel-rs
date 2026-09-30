// 搜索页：任务轮询搜索，结果渐进累计。状态在 SearchProvider，跨路由切换保留。

import { useState, useCallback, useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { Search as SearchIcon } from "lucide-react"
import { Button } from "@workspace/ui/components/button"
import { Input } from "@workspace/ui/components/input"
import { Card, CardContent } from "@workspace/ui/components/card"
import { Badge } from "@workspace/ui/components/badge"
import { Skeleton } from "@workspace/ui/components/skeleton"
import { Alert, AlertDescription } from "@workspace/ui/components/alert"
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select"
import { useSearch } from "@/hooks/use-search"
import { useSources } from "@/hooks/use-sources"
import { useTranslation } from "react-i18next"
import type { SearchResult } from "@/lib/types"

const PAGE_SIZE = 12

export default function SearchPage() {
  const [keyword, setKeyword] = useState("")
  const [sourceId, setSourceId] = useState<string>("")
  const [page, setPage] = useState(1)
  const navigate = useNavigate()
  const { t } = useTranslation()

  const { results, isFetching, searched, sourceCount, error, search: doSearch } = useSearch()
  const { data: sources = [] } = useSources()

  const totalPages = Math.max(1, Math.ceil(results.length / PAGE_SIZE))
  const paged = results.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  // 结果数变化时把页码夹回合法范围
  useEffect(() => {
    setPage((p) => Math.min(p, totalPages))
  }, [totalPages])

  // 总页数多时折叠中间页，始终保留首页 / 末页 + 当前页前后各 1 页
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

  const handleSearch = useCallback(() => {
    setPage(1)
    doSearch(keyword, sourceId ? Number(sourceId) : undefined)
  }, [keyword, sourceId, doSearch])

  return (
    <div className="flex flex-col gap-6">
      {/* 搜索栏：Input（带放大镜）+ 源下拉 + 搜索按钮。 */}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <div className="relative w-full sm:flex-1">
          <SearchIcon className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="pl-9"
            placeholder={t("search.placeholder")}
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleSearch()}
          />
        </div>
        <div className="flex items-center gap-2">
          <Select value={sourceId} onValueChange={(v) => setSourceId(v ?? "")}>
            <SelectTrigger className="w-full sm:w-36">
              <SelectValue placeholder={t("search.allSources")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="">{t("search.allSources")}</SelectItem>
              {sources
                .filter((s) => s.enabled)
                .map((s) => (
                  <SelectItem key={s.id} value={String(s.id)}>
                    {s.name}
                  </SelectItem>
                ))}
            </SelectContent>
          </Select>
          <Button onClick={handleSearch} disabled={isFetching || !keyword.trim()} className="shrink-0">
            {isFetching
              ? t("search.searching", { count: sourceCount })
              : t("search.searchButton")}
          </Button>
        </div>
      </div>

      {/* 流式错误提示 */}
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {/* 加载骨架屏 */}
      {isFetching && (
        <div className="flex flex-col gap-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-16 w-full" />
          ))}
        </div>
      )}

      {/* 搜索结果 */}
      {!isFetching && paged.length > 0 && (
        <div className="flex flex-col gap-2">
          {paged.map((r, i) => (
            <ResultCard
              key={`${r.source_id}-${r.url}-${i}`}
              result={r}
              onClick={() =>
                navigate(`/search/${encodeURIComponent(r.url)}`, { state: { sourceId: r.source_id } })
              }
            />
          ))}
          {/* 分页 */}
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
        </div>
      )}

      {/* 无结果（已搜索） */}
      {!isFetching && searched && results.length === 0 && (
        <p className="py-16 text-center text-muted-foreground">{t("search.noResults")}</p>
      )}

      {/* 未搜索初始态 */}
      {!searched && (
        <p className="py-16 text-center text-muted-foreground">{t("search.initialPrompt")}</p>
      )}
    </div>
  )
}

function ResultCard({ result: r, onClick }: { result: SearchResult; onClick: () => void }) {
  const { t } = useTranslation()
  return (
    <button
      type="button"
      onClick={onClick}
      className="block w-full rounded-xl text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <Card className="w-full transition-colors hover:bg-muted/50">
        <CardContent className="flex flex-col gap-2 p-4 sm:p-5">
          {/* 标题 + 作者 */}
          <div className="flex min-w-0 items-baseline gap-2">
            <span className="truncate text-base font-semibold">{r.book_name}</span>
            {r.author && (
              <span className="min-w-0 flex-shrink truncate text-xs text-muted-foreground">
                {r.author}
              </span>
            )}
          </div>
          {/* 简介 */}
          {r.intro && <p className="line-clamp-2 text-sm text-muted-foreground">{r.intro}</p>}
          {/* badge 行：分类 / 状态 / 字数 — 来源靠右 */}
          <div className="flex flex-wrap items-center gap-2 pt-1">
            {r.category && <Badge variant="secondary">{r.category}</Badge>}
            {r.status && <Badge variant="secondary">{r.status}</Badge>}
            {r.word_count && (
              <Badge variant="secondary" className="text-muted-foreground">
                {t("search.card.wordCount")} {r.word_count}
              </Badge>
            )}
            <span className="ml-auto text-xs text-muted-foreground/70">{r.source_name}</span>
          </div>
          {/* 最新章节 + 更新时间 */}
          {(r.latest_chapter || r.last_update_time) && (
            <div className="flex min-w-0 items-center gap-3 text-xs text-muted-foreground">
              {r.latest_chapter && (
                <span className="min-w-0 truncate">
                  <span className="text-muted-foreground/70">
                    {t("search.card.latestChapter")}:{" "}
                  </span>
                  {r.latest_chapter}
                </span>
              )}
              {r.last_update_time && (
                <span className="ml-auto flex-shrink-0 text-muted-foreground/70">
                  {r.last_update_time}
                </span>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </button>
  )
}
