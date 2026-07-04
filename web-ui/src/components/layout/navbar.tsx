// 两层 Navbar：
//   1. 顶层 — logo + app 名 + 主题切换（sticky 顶部）
//   2. 下层 — HeroUI <Tabs variant="secondary"> 作为跨页导航（underline indicator）
//
// 替代了原先「Link 列表 + 暗色按钮 + 顶部 <h1>」的散乱组合 —— 页面级的 <h1>
// 全部下放到路由组件里负责具体内容，导航语义集中在 Navbar 内。
//
// 任务计数 badge 仍然挂在 /tasks tab 上，跟原先语义一致。

import { useLocation, useNavigate } from 'react-router-dom'
import { Magnifier, ArrowDown, Book, LayoutList, Gear } from '@gravity-ui/icons'
import { Tabs, Badge } from '@heroui/react'
import ThemeToggle from '../theme-toggle'
import { useTasks } from '@/hooks/use-tasks'
import { useTranslation } from 'react-i18next'

const NAV = [
  { to: '/search',   labelKey: 'nav.search',   icon: Magnifier },
  { to: '/tasks',    labelKey: 'nav.tasks',    icon: ArrowDown },
  { to: '/library',  labelKey: 'nav.library',  icon: Book },
  { to: '/sources',  labelKey: 'nav.sources',  icon: LayoutList },
  { to: '/settings', labelKey: 'nav.settings', icon: Gear },
] as const

export default function Navbar() {
  const { pathname } = useLocation()
  const navigate = useNavigate()
  const { data: tasks = [] } = useTasks()
  const { t } = useTranslation()
  const active = tasks.filter(t => t.status === 'Downloading').length

  // 顶层路径（/search/:bookUrl 这种 detail 路由也归属到 /search tab）。
  const currentTop = '/' + (pathname.split('/').filter(Boolean)[0] ?? 'search')
  const selected = NAV.find(n => n.to === currentTop)?.to ?? '/search'

  return (
    <header className="sticky top-0 z-40 border-b bg-background/90 backdrop-blur">
      {/* 顶层：logo + app 名 + 主题切换 */}
      <div className="max-w-5xl mx-auto px-4 sm:px-6 flex items-center justify-between h-14">
        <div className="flex items-center gap-2 font-bold text-lg">
          <img src="/logo.png" alt="" className="w-6 h-6" />
          <span>{t('app.title')}</span>
        </div>
        <ThemeToggle />
      </div>

      {/* 下层：secondary Tabs 跨页导航。secondary variant 给下划线 indicator，
          跟 macOS / Linear 这类带标签栏的桌面 / web app 视觉一致。
          不用 <Link> 列表的原因是 Tabs 自带 selected 状态（aria-selected），
          键盘左右切换也免费送。 */}
      <nav className="max-w-5xl mx-auto px-4 sm:px-6">
        <Tabs
          variant="secondary"
          selectedKey={selected}
          onSelectionChange={(key) => navigate(String(key))}
        >
          <Tabs.ListContainer>
            <Tabs.List aria-label="primary-nav">
              {NAV.map(({ to, labelKey, icon: Icon }) => (
                <Tabs.Tab key={to} id={to}>
                  <Icon width={16} height={16} />
                  {t(labelKey)}
                  {to === '/tasks' && active > 0 && (
                    <Badge size="sm" color="accent">
                      {active}
                    </Badge>
                  )}
                  <Tabs.Indicator />
                </Tabs.Tab>
              ))}
            </Tabs.List>
          </Tabs.ListContainer>
        </Tabs>
      </nav>
    </header>
  )
}
