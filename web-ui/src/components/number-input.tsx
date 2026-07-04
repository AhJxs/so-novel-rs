// 数字输入 —— 封装 HeroUI v3 的 NumberField 复合组件（react-aria）。
// 受控用法：value (number) + onChange(value)。带 +/- 步进按钮。
// 各设置项的数字输入统一走这个封装，样式/主题跟随 HeroUI tokens。

import { NumberField } from '@heroui/react'
import { Minus, Plus } from '@gravity-ui/icons'

interface NumberInputProps {
  value: number
  onChange: (value: number) => void
  /** 最小值。 */
  minValue?: number
  /** 最大值。 */
  maxValue?: number
  /** 步进值，默认 1。 */
  step?: number
  isDisabled?: boolean
  'aria-label'?: string
  className?: string
}

export default function NumberInput({
  value,
  onChange,
  minValue,
  maxValue,
  step = 1,
  isDisabled,
  'aria-label': ariaLabel,
  className,
}: NumberInputProps) {
  return (
    <NumberField
      value={Number.isNaN(value) ? 0 : value}
      onChange={onChange}
      minValue={minValue}
      maxValue={maxValue}
      step={step}
      isDisabled={isDisabled}
      aria-label={ariaLabel}
      className={className}
    >
      <NumberField.Group className="w-full">
        <NumberField.DecrementButton aria-label="decrease">
          <Minus width={14} height={14} />
        </NumberField.DecrementButton>
        <NumberField.Input className="text-center" />
        <NumberField.IncrementButton aria-label="increase">
          <Plus width={14} height={14} />
        </NumberField.IncrementButton>
      </NumberField.Group>
    </NumberField>
  )
}
