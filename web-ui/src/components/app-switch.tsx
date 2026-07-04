// 通用开关 —— 封装 HeroUI v3 的 Switch 复合组件。
// HeroUI 的 <Switch> 根节点本身不渲染可见轨道/滑块，必须组合 Content/Control/Thumb。

import { Switch } from '@heroui/react'

interface AppSwitchProps {
  isSelected: boolean
  onChange: (selected: boolean) => void
  isDisabled?: boolean
  size?: 'sm' | 'md' | 'lg'
  'aria-label'?: string
}

export default function AppSwitch({
  isSelected,
  onChange,
  isDisabled,
  size = 'md',
  'aria-label': ariaLabel,
}: AppSwitchProps) {
  return (
    <Switch
      isSelected={isSelected}
      onChange={onChange}
      isDisabled={isDisabled}
      size={size}
      aria-label={ariaLabel}
    >
      <Switch.Content>
        <Switch.Control>
          <Switch.Thumb />
        </Switch.Control>
      </Switch.Content>
    </Switch>
  )
}
