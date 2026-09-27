/* @vitest-environment jsdom */
import '@/test-utils';
import { beforeEach, describe, expect, it } from 'vitest';
import { useLayoutStore } from '../layout';

describe('layout preferences', () => {
  beforeEach(() => {
    localStorage.clear();
    useLayoutStore.setState({ skillsListWidth: 320, libraryListWidth: 320 });
  });

  it('persists only a valid user-selected width', () => {
    useLayoutStore.getState().setSkillsListWidth(412.4);
    expect(useLayoutStore.getState().skillsListWidth).toBe(412);
    const stored = JSON.parse(localStorage.getItem('skill-deck-layout')!);
    expect(stored.state).toEqual({ skillsListWidth: 412, libraryListWidth: 320 });
    for (const width of [NaN, Infinity, -1, 0, 180, 100000]) {
      useLayoutStore.getState().setSkillsListWidth(width);
      expect(useLayoutStore.getState().skillsListWidth).toBe(412);
    }
  });

  it('remembers the library width independently of the installed Skills list', async () => {
    useLayoutStore.getState().setSkillsListWidth(400);
    useLayoutStore.getState().setLibraryListWidth(280);
    const stored = localStorage.getItem('skill-deck-layout')!;
    useLayoutStore.setState({ skillsListWidth: 320, libraryListWidth: 320 });
    localStorage.setItem('skill-deck-layout', stored);
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBe(400);
    expect(useLayoutStore.getState().libraryListWidth).toBe(280);
  });

  it('preserves a valid Skills preference when an older record has no library width', async () => {
    localStorage.setItem('skill-deck-layout', JSON.stringify({ state: { skillsListWidth: 400 }, version: 1 }));
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBe(400);
    expect(useLayoutStore.getState().libraryListWidth).toBe(320);
  });

  it.each([null, { skillsListWidth: -100 }, { skillsListWidth: '400' }])('ignores invalid stored preferences: %j', async (state) => {
    localStorage.setItem('skill-deck-layout', JSON.stringify({ state, version: 1 }));
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBe(320);
  });
});
