/* @vitest-environment jsdom */
import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { useSkillSourceFilter } from '../useSkillSourceFilter';
import type { SourceFilterOption } from '@/lib/skills/filter';

const source: SourceFilterOption = { key: 'repo', kind: 'git', label: 'acme/tools', detail: 'github.com', count: 2 };
describe('source filter lifetime', () => {
  it('retains selection and known options during partial reads, then keeps a selected zero-result entry', () => {
    const { result, rerender } = renderHook(({ options, incomplete }) => useSkillSourceFilter('native/project:a', options, incomplete), {
      initialProps: { options: [source], incomplete: false },
    });
    act(() => result.current.select(source));
    rerender({ options: [], incomplete: true });
    expect(result.current.options).toEqual([{ ...source, count: 0 }]);
    expect(result.current.selected?.key).toBe('repo');
    rerender({ options: [], incomplete: false });
    expect(result.current.options).toEqual([{ ...source, count: 0 }]);
    act(() => result.current.select(null));
    expect(result.current.options).toEqual([]);
  });

  it('clears source selection and retained options when the context changes', () => {
    const options = [source];
    const { result, rerender } = renderHook(({ key }) => useSkillSourceFilter(key, options, false), { initialProps: { key: 'native/global' } });
    act(() => result.current.select(source));
    rerender({ key: 'wsl:Ubuntu/global' });
    expect(result.current.selected).toBeNull();
  });
});
