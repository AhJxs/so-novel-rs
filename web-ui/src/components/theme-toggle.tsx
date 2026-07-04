import { Button } from '@heroui/react'
import { Moon, Sun, Display } from '@gravity-ui/icons'
import { useTheme } from 'next-themes'
import { useEffect, useState } from 'react'

export default function ThemeToggle() {
  const { theme, setTheme } = useTheme()
  const [mounted, setMounted] = useState(false)

  useEffect(() => setMounted(true), [])

  if (!mounted) return null

  const cycleTheme = () => {
    if (theme === 'light') setTheme('dark')
    else if (theme === 'dark') setTheme('system')
    else setTheme('light')
  }

  const Icon = theme === 'light' ? Sun : theme === 'dark' ? Moon : Display
  const size = 18

  return (
    <Button isIconOnly variant="ghost" onPress={cycleTheme} aria-label="Toggle theme">
      <Icon width={size} height={size} />
    </Button>
  )
}
