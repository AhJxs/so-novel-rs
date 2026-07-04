// 通用下拉选择器 —— 封装 HeroUI v3 的 Select 复合组件（react-aria）。
// 单选模式：selectedKey (string) + onSelectionChange(key)。
// 各页面下拉统一走这个封装，避免重复拼 Select.Trigger/Popover/ListBox。
// variant="secondary" 用于嵌在 InputGroup 内部，去掉外框让它跟输入框视觉合并。

import { Select, ListBox } from '@heroui/react'
import type { Key } from 'react'

export interface AppSelectOption {
  /** 选项值，作为 ListBox.Item 的 id。 */
  key: string
  /** 显示文本。 */
  label: string
}

interface AppSelectProps {
  /** 当前选中值。 */
  selectedKey: string
  /** 选择变化回调，返回选项 key。 */
  onChange: (key: string) => void
  options: AppSelectOption[]
  /** 未选中时的占位文本。 */
  placeholder?: string
  /** 触发器宽度类名，如 "w-36"、"w-full"。 */
  className?: string
  /** 禁用整个下拉。 */
  isDisabled?: boolean
  /** 可访问性标签（无可见 label 时建议提供）。 */
  'aria-label'?: string
  /** HeroUI 变体：primary（默认，有外框）/ secondary（无外框，适合嵌在 InputGroup）。 */
  variant?: 'primary' | 'secondary'
}

export default function AppSelect({
  selectedKey,
  onChange,
  options,
  placeholder,
  className,
  isDisabled,
  'aria-label': ariaLabel,
  variant = 'primary',
}: AppSelectProps) {
  return (
    <Select
      value={selectedKey}
      onChange={(key: Key | null) => {
        if (key != null) onChange(String(key))
      }}
      isDisabled={isDisabled}
      aria-label={ariaLabel}
      className={className}
      variant={variant}
    >
      <Select.Trigger>
        <Select.Value>
          {({ selectedText }) => selectedText || placeholder || ''}
        </Select.Value>
        <Select.Indicator />
      </Select.Trigger>
      <Select.Popover>
        <ListBox>
          {options.map((opt) => (
            <ListBox.Item key={opt.key} id={opt.key} textValue={opt.label}>
              {opt.label}
            </ListBox.Item>
          ))}
        </ListBox>
      </Select.Popover>
    </Select>
  )
}