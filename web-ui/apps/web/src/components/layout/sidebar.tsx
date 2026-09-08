// 侧边栏主导航：logo + 5 个页面导航项 + 底部主题切换。
// Base UI 变体：`render` prop 传 NavLink（非 Radix 的 asChild）。

import { NavLink, useLocation } from "react-router-dom"
import { BookOpen, ListFilter, Search, Settings, ArrowDownToLine } from "lucide-react"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@workspace/ui/components/sidebar"
import ThemeToggle from "../theme-toggle"
import { useTasks } from "@/hooks/use-tasks"
import { useTranslation } from "react-i18next"

const NAV = [
  { to: "/search", labelKey: "nav.search", icon: Search },
  { to: "/tasks", labelKey: "nav.tasks", icon: ArrowDownToLine },
  { to: "/library", labelKey: "nav.library", icon: BookOpen },
  { to: "/sources", labelKey: "nav.sources", icon: ListFilter },
  { to: "/settings", labelKey: "nav.settings", icon: Settings },
] as const

export function AppSidebar() {
  const { data: tasks = [] } = useTasks()
  const { t } = useTranslation()
  const { pathname } = useLocation()
  const active = tasks.filter((x) => x.status === "Downloading").length
  // 顶层路径：/search/:bookUrl 这类详情路由也归到 /search 项高亮。
  const activePath = "/" + (pathname.split("/").filter(Boolean)[0] ?? "search")

  return (
    <Sidebar>
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1">
          <img src="/logo.png" alt="" className="size-6" />
          <span className="font-semibold">{t("app.title")}</span>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>{t("nav.group")}</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {NAV.map(({ to, labelKey, icon: Icon }) => (
                <SidebarMenuItem key={to}>
                  <SidebarMenuButton
                    render={<NavLink to={to} end={to === "/search"} />}
                    isActive={activePath === to}
                  >
                    <Icon />
                    <span>{t(labelKey)}</span>
                    {to === "/tasks" && active > 0 && (
                      <span className="ml-auto rounded-full bg-primary px-2 text-xs text-primary-foreground">
                        {active}
                      </span>
                    )}
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <ThemeToggle />
      </SidebarFooter>
    </Sidebar>
  )
}
