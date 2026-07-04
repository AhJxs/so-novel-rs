import { Outlet } from 'react-router-dom'
import Navbar from './navbar'

/** 根布局：Navbar + 主内容区。通过 React Router 的 <Outlet> 渲染子路由。 */
export default function Layout() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <Navbar />
      <main className="max-w-5xl mx-auto px-4 sm:px-6 py-6">
        <Outlet />
      </main>
    </div>
  )
}
