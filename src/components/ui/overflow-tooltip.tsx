import type { ComponentProps, ReactElement, ReactNode, SyntheticEvent } from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from './tooltip';

/** 完整值或补充说明按需显示；尺寸在触发时读取，适应窗口和内容变化。 */
export function OverflowTooltip({ children, content, always = false, ...props }: Omit<ComponentProps<typeof TooltipContent>, 'children' | 'content'> & {
  children: ReactElement;
  content: ReactNode;
  always?: boolean;
}) {
  const guard = (event: SyntheticEvent<HTMLElement>) => {
    if (always) return;
    const element = event.currentTarget;
    const clipped = [element, ...element.querySelectorAll<HTMLElement>('.truncate, [data-tooltip-label]')]
      .some(node => node.scrollWidth > node.clientWidth
        || (node.hasAttribute('data-tooltip-label') && getComputedStyle(node).display === 'none'));
    // 提前阻止触发，避免未显示的提示也改变 Provider 的连续悬停延迟。
    if (!clipped) event.preventDefault();
  };
  return (
    <Tooltip>
      <TooltipTrigger asChild onPointerMove={guard} onFocus={guard}>{children}</TooltipTrigger>
      <TooltipContent className="max-w-[min(32rem,calc(100vw-2rem))] break-all text-wrap data-[state=closed]:animate-none!" {...props}>
        {content}
      </TooltipContent>
    </Tooltip>
  );
}
