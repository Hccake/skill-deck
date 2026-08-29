import { useCallback, useRef } from 'react';

export interface SkillCardDatePresentation {
  short: string;
  full: string;
}

export function formatSkillCardDate(
  isoString: string,
  locale?: string,
  now = new Date(),
): SkillCardDatePresentation {
  const date = new Date(isoString);
  if (Number.isNaN(date.getTime())) {
    return { short: isoString, full: isoString };
  }

  const sameDay = date.getFullYear() === now.getFullYear()
    && date.getMonth() === now.getMonth()
    && date.getDate() === now.getDate();
  const shortOptions: Intl.DateTimeFormatOptions = sameDay
    ? { hour: '2-digit', minute: '2-digit', hour12: false }
    : date.getFullYear() === now.getFullYear()
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' };

  return {
    short: new Intl.DateTimeFormat(locale, shortOptions).format(date),
    full: new Intl.DateTimeFormat(locale, {
      dateStyle: 'long',
      timeStyle: 'short',
    }).format(date),
  };
}

/** 只有能用系统浏览器打开的地址才渲染成链接。 */
export function isOpenableUrl(value: string | null | undefined): value is string {
  if (!value) return false;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' || url.protocol === 'http:';
  } catch {
    return false;
  }
}

/**
 * 整卡点击打开详情的鼠标便利路径。
 *
 * 卡片根节点保持普通 `div`：卡内已有标题、来源和操作按钮，给根节点加 `button` 角色会形成
 * 嵌套交互元素。键盘与辅助技术走标题按钮，这里只处理鼠标，并让用户仍能选中描述文字——
 * 划过一段距离或已有选区时不触发。
 */
export function useCardActivation(onActivate: (() => void) | undefined) {
  const pointerDownRef = useRef<{ x: number; y: number } | null>(null);

  const onPointerDown = useCallback((event: React.PointerEvent<HTMLElement>) => {
    pointerDownRef.current = { x: event.clientX, y: event.clientY };
  }, []);

  const onClick = useCallback((event: React.MouseEvent<HTMLElement>) => {
    if (window.getSelection()?.toString()) return;
    const pointerDown = pointerDownRef.current;
    pointerDownRef.current = null;
    if (pointerDown) {
      const distance = Math.hypot(event.clientX - pointerDown.x, event.clientY - pointerDown.y);
      if (distance > 4) return;
    }
    onActivate?.();
  }, [onActivate]);

  return { onPointerDown, onClick };
}
