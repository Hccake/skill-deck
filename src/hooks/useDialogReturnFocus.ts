import { useCallback, useRef } from 'react';

/** 由状态打开的维护弹窗没有 DialogTrigger，显式保留其实际发起入口。 */
export function useDialogReturnFocus() {
  const trigger = useRef<HTMLElement | null>(null);
  const onOpenAutoFocus = useCallback(() => {
    trigger.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  }, []);
  const onCloseAutoFocus = useCallback((event: Event) => {
    if (trigger.current?.isConnected && !trigger.current.matches(':disabled')) {
      event.preventDefault();
      trigger.current.focus({ preventScroll: true });
    }
  }, []);
  return { onOpenAutoFocus, onCloseAutoFocus };
}
