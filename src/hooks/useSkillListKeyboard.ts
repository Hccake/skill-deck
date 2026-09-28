import { useLayoutEffect, useRef, type FocusEvent, type KeyboardEvent } from 'react';

const ENTRY = 'button[data-skill-open]';
const SEARCH = 'input[data-skill-search]';
const keyOf = (node: Element) => node.closest<HTMLElement>('[data-skill-item]')?.dataset.skillItem;
const entriesOf = (root: HTMLElement) => Array.from(root.querySelectorAll<HTMLButtonElement>(ENTRY));

function focusEntry(node: HTMLElement | null | undefined) {
  node?.focus({ preventScroll: true });
  node?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
}

/** 两类管理列表共用键盘入口；对象键只在当前 Environment 与 Scope/库内解释。 */
export function useSkillListKeyboard({ compact, selectedKey, contextKey }: {
  compact: boolean;
  selectedKey: string | null;
  contextKey: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const previous = useRef({ compact, contextKey });
  const focused = useRef<{ key?: string; index: number; node: HTMLElement | null }>({ index: 0, node: null });
  const selection = useRef<{ key: string | null; index: number }>({ key: selectedKey, index: 0 });

  const updateTabStop = (root: HTMLElement, preferred?: string | null) => {
    if (!compact) return;
    const entries = entriesOf(root);
    const target = entries.find((entry) => keyOf(entry) === preferred)
      ?? entries.find((entry) => keyOf(entry) === selectedKey)
      ?? entries.find((entry) => keyOf(entry) === focused.current.key)
      ?? entries[0];
    for (const entry of entries) entry.tabIndex = entry === target ? 0 : -1;
  };

  // 在列表真实提交后处理模式切换和被移除的入口，避免计时器与过滤、异步刷新争抢焦点。
  useLayoutEffect(() => {
    const before = previous.current;
    previous.current = { compact, contextKey };
    const contextChanged = before.contextKey !== contextKey;
    if (contextChanged) {
      focused.current = { index: 0, node: null };
      selection.current = { key: null, index: 0 };
    }
    const root = ref.current;
    if (!root) return;
    const entries = entriesOf(root);
    const selectedIndex = entries.findIndex((entry) => keyOf(entry) === selectedKey);
    if (selectedKey && !contextChanged) {
      selection.current = { key: selectedKey, index: selectedIndex >= 0 ? selectedIndex : selection.current.index };
    }
    const active = document.activeElement;
    const modeChanged = !contextChanged && before.compact !== compact;
    const lostEntry = focused.current.node && !focused.current.node.isConnected
      && (!active || active === document.body);
    const overlayActive = active instanceof Element && active.closest('[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"]');
    if (!contextChanged && !overlayActive && (modeChanged || lostEntry)) {
      const key = modeChanged ? selection.current.key : focused.current.key;
      const index = modeChanged ? selection.current.index : focused.current.index;
      focusEntry(entries.find((entry) => keyOf(entry) === key)
        ?? entries[Math.min(index, entries.length - 1)]
        ?? root.querySelector<HTMLInputElement>(SEARCH)
        ?? root.querySelector<HTMLButtonElement>('button:not(:disabled)'));
    }
    const activeEntry = document.activeElement;
    updateTabStop(root, activeEntry instanceof Element && activeEntry.matches(ENTRY) && root.contains(activeEntry)
      ? keyOf(activeEntry) : null);
  });

  const onFocusCapture = (event: FocusEvent<HTMLDivElement>) => {
    const target = event.target;
    if (!event.currentTarget.contains(target)) return;
    const key = keyOf(target);
    if (key) {
      const entries = entriesOf(event.currentTarget);
      focused.current = { key, index: Math.max(0, entries.findIndex((entry) => keyOf(entry) === key)), node: target };
      if (target.matches(ENTRY)) updateTabStop(event.currentTarget, key);
    } else {
      focused.current.node = null;
    }
  };

  const onBlurCapture = (event: FocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.target)) return;
    const next = event.relatedTarget;
    if (next instanceof HTMLElement && next !== document.body) {
      focused.current.node = null;
      if (!event.currentTarget.contains(next) || !next.matches(ENTRY)) updateTabStop(event.currentTarget);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.defaultPrevented || event.nativeEvent.isComposing || event.keyCode === 229
      || !(event.target instanceof HTMLElement) || !event.currentTarget.contains(event.target)) return;
    if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'f') {
      const search = event.currentTarget.querySelector<HTMLInputElement>(SEARCH);
      if (search) {
        event.preventDefault();
        event.stopPropagation();
        search.focus();
        search.select();
      }
      return;
    }
    if (!compact || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || !event.target.matches(ENTRY)) return;
    const entries = entriesOf(event.currentTarget);
    const index = entries.indexOf(event.target as HTMLButtonElement);
    const next = event.key === 'ArrowDown' ? Math.min(index + 1, entries.length - 1)
      : event.key === 'ArrowUp' ? Math.max(index - 1, 0)
      : event.key === 'Home' ? 0
      : event.key === 'End' ? entries.length - 1 : null;
    if (next === null) return;
    event.preventDefault();
    event.stopPropagation();
    focusEntry(entries[next]);
  };

  return { ref, onFocusCapture, onBlurCapture, onKeyDown };
}
