// 书籍详情页 —— 详情展示 + 目录获取。
// 点击下载直接 start + 跳转到 /tasks，进度在任务页里看（后端任务独立于组件生命周期）。
// bookUrl 来自 URL params，sourceId 由搜索页通过 navigate state 传入。

import { useState } from 'react'
import { useParams, useNavigate, useLocation } from 'react-router-dom'
import { ChevronLeft } from '@gravity-ui/icons'
import { Button, Chip, Skeleton, ButtonGroup } from '@heroui/react'
import { useTranslation } from 'react-i18next'
import { useBookDetail, useToc } from '@/hooks/use-book'
import { useDownload } from '@/hooks/use-download'
import type { ExportFormat } from '@/lib/types'

export default function BookDetailPage() {
  const { bookUrl } = useParams<{ bookUrl: string }>()
  const navigate = useNavigate()
  const location = useLocation()
  const { t } = useTranslation()
  const decoded = decodeURIComponent(bookUrl ?? '')
  const sourceId = (location.state as { sourceId?: number } | null)?.sourceId ?? null

  const { data: book, isLoading } = useBookDetail(decoded, sourceId)
  const { data: toc, refetch: loadToc, isFetching: loadingToc } = useToc(decoded, sourceId)
  const chapters = toc?.chapters ?? []
  const { start: startDl } = useDownload()

  const [format, setFormat] = useState<ExportFormat>('epub')

  // 启动下载：等后端把任务 push 到 state.tasks（即 `startDownload.started` resolve）
  // 才跳任务页。`useTasks` 的 `refetchInterval` 只有在缓存里看到 Downloading 任务
  // 才会轮询；立即跳转可能让 tasks 页拿着无 Downloading 的旧缓存 + 没到位的
  // refetch，就看不到新任务。await `startDl(...)` 让 useDownload 在 onStarted
  // 时 invalidate ['tasks']，跳转瞬间 refetch 已经在进行 + 后端一定已入库。
  const handleDownload = async () => {
    if (!bookUrl || sourceId == null) return
    await startDl({ url: decoded, sourceId, format })
    navigate('/tasks')
  }

  if (sourceId == null) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onPress={() => navigate(-1)}>
          <ChevronLeft className="w-4 h-4 mr-1" /> {t('book.backToSearch')}
        </Button>
        <p className="text-center py-16 text-default-500">{t('book.missingSource')}</p>
      </div>
    )
  }

  if (isLoading) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onPress={() => navigate(-1)}>
          <ChevronLeft className="w-4 h-4 mr-1" /> {t('book.backToSearch')}
        </Button>
        <Skeleton className="h-64 w-full rounded-2xl" />
      </div>
    )
  }

  if (!book) {
    return (
      <div className="space-y-4">
        <Button variant="ghost" size="sm" onPress={() => navigate(-1)}>
          <ChevronLeft className="w-4 h-4 mr-1" /> {t('book.backToSearch')}
        </Button>
        <p className="text-center py-16 text-default-500">{t('book.loadFailed')}</p>
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <Button variant="ghost" size="sm" onPress={() => navigate(-1)}>
        <ChevronLeft className="w-4 h-4 mr-1" /> {t('book.backToSearch')}
      </Button>

      {/* 书籍信息卡片 */}
      <div className="bg-surface border rounded-2xl overflow-hidden">
        {/* 基本信息 */}
        <div className="p-6 flex gap-6">
          <div className="w-24 h-32 rounded-xl flex-shrink-0 overflow-hidden bg-default relative">
            {book.cover_url
              ? <img src={book.cover_url} alt={book.book_name} referrerPolicy="no-referrer"
                  className="absolute inset-0 w-full h-full object-cover" />
              : <div className="absolute inset-0 bg-gradient-to-br from-violet-400 to-purple-600
                  flex items-center justify-center text-white text-3xl font-bold">{book.book_name[0]}</div>
            }
          </div>
          <div className="flex-1 min-w-0">
            <h2 className="text-xl font-bold">{book.book_name}</h2>
            <p className="text-default-500 text-sm mt-1">{book.author}</p>
            {book.intro && <p className="text-sm mt-3 line-clamp-3 text-default-500">{book.intro}</p>}
            <div className="flex gap-2 mt-3 flex-wrap">
              {book.status && <Chip size="sm" variant="soft" className="text-success">{book.status}</Chip>}
              {book.latest_chapter && <Chip size="sm" variant="soft">{t('book.latestChapter')}: {book.latest_chapter}</Chip>}
            </div>
          </div>
        </div>

        {/* 目录 */}
        <div className="border-t p-6">
          <div className="flex items-center justify-between mb-3">
            <h3 className="font-semibold">
              {t('book.toc')}
              {chapters.length > 0 && <span className="text-sm text-default-500 ml-1">({t('book.chapters', { count: chapters.length })})</span>}
            </h3>
            <Button variant="ghost" size="sm" onPress={() => loadToc()} isDisabled={loadingToc}>
              {loadingToc ? t('book.loading') : chapters.length ? t('book.refreshToc') : t('book.loadToc')}
            </Button>
          </div>
          {chapters.length > 0 && (
            <div className="h-48 overflow-y-auto">
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-1">
                {chapters.slice(0, 200).map(ch => (
                  <div key={ch.order}
                    className="text-xs px-2 py-1 rounded truncate text-default-500 hover:bg-default-100">
                    {ch.order}. {ch.title}
                  </div>
                ))}
              </div>
            </div>
          )}
          {!loadingToc && chapters.length === 0 && (
            <p className="text-xs text-default-500">{t('book.notLoaded')}</p>
          )}
        </div>

        {/* 下载：选格式 + 启动（跳转到任务页看进度） */}
        <div className="border-t p-6 bg-default-50 dark:bg-default-100/20">
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm font-medium">{t('book.format')}</span>
            <ButtonGroup>
              {(['epub','txt','html','pdf','markdown'] as ExportFormat[]).map(f => (
                <Button
                  key={f}
                  size="sm"
                  variant={format === f ? 'primary' : 'ghost'}
                  onPress={() => setFormat(f)}
                  className="uppercase text-xs"
                >
                  {f}
                </Button>
              ))}
            </ButtonGroup>
            <Button className="ml-auto" variant="primary" onPress={handleDownload}>
              {t('book.startDownload')}
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
