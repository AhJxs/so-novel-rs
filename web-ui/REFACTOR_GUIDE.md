# HeroUI 重构实施指南

## 当前状态
- ✅ 基础设施 100%
- ✅ i18n 系统 100%  
- ⚠️ 组件重构 40% (Search页面完成，5个页面待完成)

## 关键发现

### 1. HeroUI 组件导入方式
```tsx
// ✅ 正确
import { Button, Card, Input, Chip } from '@heroui/react'

// ❌ 错误
import { NextUIProvider } from '@heroui/react' // 不存在
import { CardBody } from '@heroui/react' // 不存在

// Card 使用方式
<Card>
  <Card.Header>...</Card.Header>
  <Card.Body>...</Card.Body>  {/* 或者直接用 children */}
</Card>
```

### 2. HeroUI Props 差异
```tsx
// Button
<Button 
  variant="primary" // primary, secondary, tertiary, ghost, outline, danger, danger-soft
  onPress={() => {}} // 不是 onClick
  isDisabled={true} // 不是 disabled
/>

// Input
<Input
  value={value}
  onChange={(e) => setValue(e.target.value)} // 不是 onValueChange
  isDisabled={true}
/>

// Chip (替代 Badge)
<Chip 
  variant="primary" // primary, secondary, tertiary, soft
  size="sm"
>
  文本
</Chip>
```

### 3. gravity-ui/icons 图标映射
```tsx
// ✅ 可用图标
import { 
  Magnifier,      // 搜索
  ArrowDown,      // 下载
  Book,           // 书籍
  Gear,           // 设置
  Moon, Sun, Display, // 主题
  ChevronLeft,    // 左箭头
  TrashBin,       // 删除
  Thunderbolt,    // 测速
  CircleCheck,    // 成功
  CircleXmark,    // 错误
  Ban,            // 取消
} from '@gravity-ui/icons'

// ❌ 不可用 - 需要替代
// Rss -> 使用 Magnifier 或 找相似图标
// Loader2 -> 使用 CSS 动画 + 任意图标
```

## 待完成页面的重构模板

### Book Detail 页面
需要替换的组件：
- `ChevronLeft` from lucide-react -> `ChevronLeft` from @gravity-ui/icons
- Button -> HeroUI Button (variant, onPress, isDisabled)
- Progress -> HeroUI ProgressBar
- Badge -> HeroUI Chip
- Skeleton -> HeroUI Skeleton
- ToggleGroup -> 使用 ButtonGroup 自行实现

关键改动：
```tsx
// 旧
<Button variant="ghost" size="sm" onClick={...}>

// 新
<Button variant="ghost" size="sm" onPress={...}>

// 旧
<Badge variant="outline">

// 新  
<Chip variant="secondary">

// 旧
<ToggleGroup type="single" value={format} onValueChange={...}>
  <ToggleGroupItem value="epub">EPUB</ToggleGroupItem>
</ToggleGroup>

// 新 - 使用 ButtonGroup
import { ButtonGroup } from '@heroui/react'
<ButtonGroup>
  {['epub', 'txt', 'html', 'pdf'].map(f => (
    <Button 
      key={f}
      variant={format === f ? 'primary' : 'ghost'}
      onPress={() => setFormat(f)}
    >
      {f.toUpperCase()}
    </Button>
  ))}
</ButtonGroup>
```

### Tasks 页面
需要替换：
- lucide-react 图标 -> @gravity-ui/icons
- Card -> HeroUI Card
- Button -> HeroUI Button
- Progress -> HeroUI ProgressBar  
- Badge -> HeroUI Chip

### Library 页面
需要替换：
- lucide-react 图标 -> @gravity-ui/icons
- Card -> HeroUI Card
- Button -> HeroUI Button
- Tabs -> HeroUI Tabs

Tabs 改动：
```tsx
// 旧
<Tabs value={ext} onValueChange={setExt}>
  <TabsList>
    <TabsTrigger value="all">全部</TabsTrigger>
  </TabsList>
</Tabs>

// 新
<Tabs selectedKey={ext} onSelectionChange={(key) => setExt(key as string)}>
  <Tabs.List>
    <Tabs.Tab key="all">全部</Tabs.Tab>
    <Tabs.Tab key="epub">EPUB</Tabs.Tab>
  </Tabs.List>
</Tabs>
```

### Sources 页面
需要替换：
- lucide-react -> @gravity-ui/icons (Zap -> Thunderbolt)
- Card -> HeroUI Card
- Switch -> HeroUI Switch
- Button -> HeroUI Button
- Badge -> HeroUI Chip

Switch 改动：
```tsx
// 旧
<Switch checked={enabled} onCheckedChange={toggle} />

// 新
<Switch isSelected={enabled} onValueChange={toggle} />
```

### Settings 页面 (最复杂)
需要替换：
- 所有 UI 组件 -> HeroUI 组件
- 添加语言选择器
- 连接后端 language 字段

新增语言选择器：
```tsx
import { Select, SelectItem } from '@heroui/react'
import { useTranslation } from 'react-i18next'
import { languageToLocale, localeToLanguage, type Locale } from '@/lib/language'

function LanguageSelector() {
  const { i18n } = useTranslation()
  const { mutate: saveSettings } = useSaveSettings()
  
  const handleChange = (locale: Locale) => {
    i18n.changeLanguage(locale)
    const backendLang = localeToLanguage(locale)
    saveSettings({ language: backendLang })
  }
  
  return (
    <Select
      label="界面语言"
      selectedKeys={[i18n.language]}
      onSelectionChange={(keys) => {
        const locale = Array.from(keys)[0] as Locale
        handleChange(locale)
      }}
    >
      <SelectItem key="zh-CN">简体中文</SelectItem>
      <SelectItem key="zh-TW">繁體中文</SelectItem>
      <SelectItem key="en">English</SelectItem>
    </Select>
  )
}
```

## 快速修复检查清单

每个页面重构时检查：
- [ ] 移除 lucide-react 导入
- [ ] 导入 @gravity-ui/icons 对应图标
- [ ] 移除 @/components/ui/* 导入
- [ ] 导入 @heroui/react 组件
- [ ] 添加 useTranslation() hook
- [ ] 替换所有硬编码文本为 t('key')
- [ ] Button: onClick -> onPress, disabled -> isDisabled
- [ ] Input: onValueChange -> onChange
- [ ] Badge -> Chip
- [ ] variant 属性值检查
- [ ] 测试编译通过

## main.tsx 修复

```tsx
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from 'next-themes'
import { Toaster } from 'sonner'
import App from './App'
import './i18n'
import './index.css'

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, retry: 1 } },
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <ThemeProvider attribute="class" defaultTheme="system">
          <App />
          <Toaster richColors position="top-right" />
        </ThemeProvider>
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
)
```

注意：不需要 NextUIProvider，HeroUI 组件可以直接使用。

## 预估工作量

- Book Detail: 30-45分钟
- Tasks: 20-30分钟
- Library: 20-30分钟
- Sources: 15-20分钟
- Settings: 45-60分钟 (包含语言选择器)
- 测试和调试: 30-60分钟

**总计**: 约 3-4 小时

## 建议执行顺序

1. 修复 main.tsx (移除 NextUIProvider)
2. 修复 Navbar (替换 Rss 图标)
3. Sources 页面 (最简单)
4. Tasks 页面
5. Library 页面
6. Book Detail 页面
7. Settings 页面 (最复杂)
8. 全局测试和优化

## 后续优化

完成基本重构后：
- 移动端响应式优化
- 添加骨架屏加载状态
- 优化空状态设计
- 性能优化
- 无障碍测试
