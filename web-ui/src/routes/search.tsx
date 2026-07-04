// 搜索页 —— SSE 流式搜索，结果逐源累加。状态在 SearchProvider 里，
// 跨路由切换（去书库/任务等再回来）保留已加载的结果。

import { useState, useCallback, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button, SearchField, Card, Chip, Skeleton, Pagination } from '@heroui/react'
import AppSelect from '@/components/app-select'
import { useSearch } from '@/hooks/use-search'
import { useSources } from '@/hooks/use-sources'
import { useTranslation } from 'react-i18next'
import type { SearchResult } from '@/lib/types'

const PAGE_SIZE = 12

export default function SearchPage() {
  const [keyword, setKeyword] = useState('')
  const [sourceId, setSourceId] = useState<string>('')  // 显示用 string
  const [page, setPage] = useState(1)
  const navigate = useNavigate()
  const { t } = useTranslation()

  const { results, isFetching, searched, sourceCount, error, search: doSearch } = useSearch()
  const { data: sources = [] } = useSources()

  const totalPages = Math.max(1, Math.ceil(results.length / PAGE_SIZE))
  const paged = results.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  // 文件数变化（如新一轮搜索结果数变少）时把页码夹回合法范围。
  useEffect(() => {
    setPage(p => Math.min(p, totalPages))
  }, [totalPages])

  // 总页数多时折叠中间页：始终保留首页 / 末页 + 当前页前后各 1 页，中间用 … 代替。
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

  const handleSearch = useCallback(() => {
    setPage(1)
    doSearch(keyword, sourceId ? Number(sourceId) : undefined)
  }, [keyword, sourceId, doSearch])

  return (
    <div className="space-y-6">
      {/* 搜索栏：SearchField（自带放大镜 + 清空按钮）+ 源下拉 + 搜索按钮，三个独立控件。
          响应式布局：
            - 小屏（< sm）：SearchField 占满第一行；源下拉 + 搜索按钮在第二行右对齐
              —— 源下拉 flex-1 撑开，按钮 shrink-0 贴右，保证两个控件垂直对齐
            - sm+：恢复横向 1 行（SearchField flex-1 + 源下拉固定 w-36 + 按钮） */}
      <div className="flex flex-col sm:flex-row sm:items-center gap-2">
        <SearchField
          className="w-full sm:flex-1"
          value={keyword}
          onChange={setKeyword}
        >
          <SearchField.Group>
            <SearchField.SearchIcon />
            <SearchField.Input
              placeholder={t('search.placeholder')}
              onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            />
            <SearchField.ClearButton />
          </SearchField.Group>
        </SearchField>
        {/* 小屏下把 select + button 包到子行：select flex-1 撑开剩余，button 固定。
            sm+ 下这层 wrapper 退化为普通 flex row，跟外层 flex-row 视觉一致 —— children
            横向排列、跟 SearchField 同一基线。 */}
        <div className="flex items-center gap-2">
          <AppSelect
            className="w-full sm:w-36"
            aria-label={t('search.allSources')}
            selectedKey={sourceId}
            onChange={(key) => setSourceId(key)}
            options={[
              { key: '', label: t('search.allSources') },
              ...sources.filter(s => s.enabled).map(s => ({ key: String(s.id), label: s.name })),
            ]}
          />
          {/* isDisabled 三种条件取或：正在请求 / 输入框为空 / 只有空白字符。
              空白 trim 跟 handleSearch 内 sendQuery 行为对齐 —— trim 后空串后端会
              直接报 400 或返回空列表，提前在 UI 阻止更友好。 */}
          <Button
            variant="primary"
            onPress={handleSearch}
            isDisabled={isFetching || !keyword.trim()}
            className="shrink-0"
          >
            {isFetching ? t('search.searching', { count: sourceCount }) : t('search.searchButton')}
          </Button>
        </div>
      </div>

      {/* 流式错误提示 */}
      {error && (
        <p className="text-sm text-danger">{error}</p>
      )}

      {/* 加载骨架屏 */}
      {isFetching && (
        <div className="space-y-2">
          {Array.from({ length: 5 }).map((_, i) => (
            <Skeleton key={i} className="h-16 w-full rounded-xl" />
          ))}
        </div>
      )}

      {/* 搜索结果 */}
      {!isFetching && paged.length > 0 && (
        <div className="space-y-2">
          {paged.map((r, i) => (
            <ResultCard key={`${r.source_id}-${r.url}-${i}`} result={r}
              onClick={() => navigate(`/search/${encodeURIComponent(r.url)}`, { state: { sourceId: r.source_id } })} />
          ))}
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
                    n === 'ellipsis'
                      ? (
                        <Pagination.Item key={`e-${i}`}>
                          <Pagination.Ellipsis />
                        </Pagination.Item>
                      )
                      : (
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
        </div>
      )}

      {/* 无结果（已搜索） */}
      {!isFetching && searched && results.length === 0 && (
        <p className="text-center py-16 text-default-500">{t('search.noResults')}</p>
      )}

      {/* 未搜索初始态 */}
      {!searched && (
        <p className="text-center py-16 text-default-500">{t('search.initialPrompt')}</p>
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
      className="block w-full text-left rounded-xl focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    >
      <Card className="px-5 py-4 w-full transition-colors hover:bg-default-100 dark:hover:bg-default-50/40">
        <div className="space-y-2">
          {/* 标题 + 作者 */}
          <div className="flex items-baseline gap-2 min-w-0">
            <span className="font-semibold text-base truncate">{r.book_name}</span>
            {r.author && (
              <span className="text-xs text-default-500 truncate flex-shrink min-w-0">{r.author}</span>
            )}
          </div>
          {/* 简介 */}
          {r.intro && (
            <p className="text-sm text-default-500 line-clamp-2">{r.intro}</p>
          )}
          {/* chip 行：分类 / 状态 / 字数 — 来源靠右 */}
          <div className="flex items-center gap-2 pt-1 flex-wrap">
            {r.category && <Chip size="sm" variant="soft">{r.category}</Chip>}
            {r.status && <Chip size="sm" variant="soft" className="text-success">{r.status}</Chip>}
            {r.word_count && (
              <Chip size="sm" variant="soft" className="text-default-500">
                {t('search.card.wordCount')} {r.word_count}
              </Chip>
            )}
            <span className="ml-auto text-xs text-default-400">{r.source_name}</span>
          </div>
          {/* 最新章节 + 更新时间 */}
          {(r.latest_chapter || r.last_update_time) && (
            <div className="flex items-center gap-3 text-xs text-default-500 min-w-0">
              {r.latest_chapter && (
                <span className="truncate min-w-0">
                  <span className="text-default-400">{t('search.card.latestChapter')}: </span>
                  {r.latest_chapter}
                </span>
              )}
              {r.last_update_time && (
                <span className="ml-auto flex-shrink-0 text-default-400">{r.last_update_time}</span>
              )}
            </div>
          )}
        </div>
      </Card>
    </button>
  )
}