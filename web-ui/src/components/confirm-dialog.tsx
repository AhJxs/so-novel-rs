// 确认对话框 —— 封装 HeroUI v3 的 AlertDialog（react-aria Modal）。
// 受控用法：isOpen + onOpenChange。确认走 onConfirm，取消/关闭自动收起。
// 危险操作（删除等）默认 status="danger"、确认按钮红色。

import { AlertDialog, Button } from '@heroui/react'
import type { ReactNode } from 'react'

interface ConfirmDialogProps {
  isOpen: boolean
  onOpenChange: (open: boolean) => void
  title: string
  message: ReactNode
  confirmLabel: string
  cancelLabel: string
  /** 确认回调。调用后对话框自动关闭。 */
  onConfirm: () => void
  /** 确认按钮语义色，默认 danger（删除类）。 */
  confirmVariant?: 'primary' | 'danger'
}

export default function ConfirmDialog({
  isOpen,
  onOpenChange,
  title,
  message,
  confirmLabel,
  cancelLabel,
  onConfirm,
  confirmVariant = 'danger',
}: ConfirmDialogProps) {
  return (
    <AlertDialog isOpen={isOpen} onOpenChange={onOpenChange}>
      <AlertDialog.Backdrop>
        <AlertDialog.Container placement="center" size="sm">
          <AlertDialog.Dialog>
            {({ close }: { close: () => void }) => (
              <>
                <AlertDialog.Header>
                  <AlertDialog.Icon status="danger" />
                  <AlertDialog.Heading>{title}</AlertDialog.Heading>
                </AlertDialog.Header>
                <AlertDialog.Body>{message}</AlertDialog.Body>
                <AlertDialog.Footer>
                  <Button variant="ghost" onPress={close}>
                    {cancelLabel}
                  </Button>
                  <Button
                    variant={confirmVariant}
                    onPress={() => {
                      onConfirm()
                      close()
                    }}
                  >
                    {confirmLabel}
                  </Button>
                </AlertDialog.Footer>
              </>
            )}
          </AlertDialog.Dialog>
        </AlertDialog.Container>
      </AlertDialog.Backdrop>
    </AlertDialog>
  )
}
