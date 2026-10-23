import { useState } from 'react';
import type { SourceFilterOption } from '@/lib/skills/filter';

/** 部分读取保留来源选项；跨管理位置清空选择，完整刷新后仍保留已选零结果项。 */
export function useSkillSourceFilter(contextKey: string, current: SourceFilterOption[], incomplete: boolean) {
  const [state, setState] = useState<{
    contextKey: string;
    observed: SourceFilterOption[];
    incomplete: boolean;
    known: SourceFilterOption[];
    selected: SourceFilterOption | null;
  }>({ contextKey, observed: current, incomplete, known: current, selected: null });

  if (state.contextKey !== contextKey) {
    setState({ contextKey, observed: current, incomplete, known: current, selected: null });
  } else if (state.observed !== current || state.incomplete !== incomplete) {
    const known = incomplete
      ? [...new Map([...state.known, ...current].map(option => [option.key, option])).values()]
      : current;
    setState({ ...state, observed: current, incomplete, known });
  }

  const selected = state.contextKey === contextKey ? state.selected : null;
  const counts = new Map(current.map(option => [option.key, option.count]));
  const options = state.known.map(option => ({ ...option, count: counts.get(option.key) ?? 0 }));
  if (selected && !options.some(option => option.key === selected.key)) options.push({ ...selected, count: 0 });
  return {
    selected,
    options,
    select: (option: SourceFilterOption | null) => setState(previous => ({ ...previous, selected: option })),
  };
}
