import * as React from "react"
import { Popover as PopoverPrimitive } from "radix-ui"

import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"

interface PopConfirmProps {
  children: React.ReactNode
  title: React.ReactNode
  description?: React.ReactNode
  confirmLabel: React.ReactNode
  cancelLabel: React.ReactNode
  onConfirm: () => void | Promise<void>
  className?: string
  align?: React.ComponentProps<typeof PopoverPrimitive.Content>["align"]
  side?: React.ComponentProps<typeof PopoverPrimitive.Content>["side"]
}

function PopConfirm({
  children,
  title,
  description,
  confirmLabel,
  cancelLabel,
  onConfirm,
  className,
  align = "end",
  side = "bottom",
}: PopConfirmProps) {
  const [open, setOpen] = React.useState(false)

  const handleConfirm = React.useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      event.stopPropagation()
      setOpen(false)
      void onConfirm()
    },
    [onConfirm]
  )

  return (
    <PopoverPrimitive.Root open={open} onOpenChange={setOpen}>
      <PopoverPrimitive.Trigger asChild>
        {children}
      </PopoverPrimitive.Trigger>
      <PopoverPrimitive.Portal>
        <PopoverPrimitive.Content
          role="dialog"
          align={align}
          side={side}
          sideOffset={6}
          className={cn(
            "z-50 w-64 rounded-md border bg-popover p-3 text-popover-foreground shadow-md outline-none",
            "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95",
            "data-[side=bottom]:slide-in-from-top-2 data-[side=top]:slide-in-from-bottom-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2",
            className
          )}
          onClick={(event) => event.stopPropagation()}
        >
          <div className="space-y-1">
            <div className="text-sm font-semibold leading-none text-foreground">
              {title}
            </div>
            {description ? (
              <div className="text-xs leading-relaxed text-muted-foreground">
                {description}
              </div>
            ) : null}
          </div>
          <div className="mt-3 flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              size="xs"
              className="cursor-pointer"
              onClick={(event) => {
                event.stopPropagation()
                setOpen(false)
              }}
            >
              {cancelLabel}
            </Button>
            <Button
              type="button"
              size="xs"
              className="cursor-pointer"
              onClick={handleConfirm}
            >
              {confirmLabel}
            </Button>
          </div>
        </PopoverPrimitive.Content>
      </PopoverPrimitive.Portal>
    </PopoverPrimitive.Root>
  )
}

export { PopConfirm }
