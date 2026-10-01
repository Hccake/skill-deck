/* @vitest-environment jsdom */
import '@/test-utils';
import { beforeEach, describe, expect, it } from 'vitest';
import { useLayoutStore } from '../layout';

describe('layout preferences', () => {
  it('restores Discover independently and ignores invalid user widths', async () => {
    useLayoutStore.getState().setDiscoverListWidth(480.4);
    useLayoutStore.getState().setSkillsListWidth(400);
    useLayoutStore.getState().setLibraryListWidth(280);
    for (const width of [NaN, Infinity, -1, 0, 239, 100000]) {
      useLayoutStore.getState().setDiscoverListWidth(width);
      expect(useLayoutStore.getState().discoverListWidth).toBe(480);
    }
    const stored = localStorage.getItem('skill-deck-layout')!;
    useLayoutStore.setState({ discoverListWidth: null, skillsListWidth: null, libraryListWidth: null });
    localStorage.setItem('skill-deck-layout', stored);
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState()).toMatchObject({ discoverListWidth: 480, skillsListWidth: 400, libraryListWidth: 280 });
  });

  it.each([{}, { discoverListWidth: -100 }, { discoverListWidth: '400' }])('defaults Discover to automatic for older or invalid data: %j', async (extra) => {
    localStorage.setItem('skill-deck-layout', JSON.stringify({ state: { skillsListWidth: 360, ...extra }, version: 2 }));
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().discoverListWidth).toBeNull();
    expect(useLayoutStore.getState().skillsListWidth).toBe(360);
  });

  beforeEach(() => {
    localStorage.clear();
    useLayoutStore.setState({ skillsListWidth: null, libraryListWidth: null, discoverListWidth: null, skillsSidebarWidth: 208, librarySidebarWidth: 208 });
  });

  it('starts both workspaces in automatic sizing mode', () => {
    expect(useLayoutStore.getInitialState().skillsListWidth).toBeNull();
    expect(useLayoutStore.getInitialState().libraryListWidth).toBeNull();
    expect(useLayoutStore.getInitialState().discoverListWidth).toBeNull();
  });

  it('persists only a valid user-selected width', () => {
    useLayoutStore.getState().setSkillsListWidth(412.4);
    expect(useLayoutStore.getState().skillsListWidth).toBe(412);
    const stored = JSON.parse(localStorage.getItem('skill-deck-layout')!);
    expect(stored.state).toEqual({ skillsListWidth: 412, libraryListWidth: null, discoverListWidth: null, skillsSidebarWidth: 208, librarySidebarWidth: 208 });
    for (const width of [NaN, Infinity, -1, 0, 180, 100000]) {
      useLayoutStore.getState().setSkillsListWidth(width);
      expect(useLayoutStore.getState().skillsListWidth).toBe(412);
    }
  });

  it('remembers the library width independently of the installed Skills list', async () => {
    useLayoutStore.getState().setSkillsListWidth(400);
    useLayoutStore.getState().setLibraryListWidth(280);
    const stored = localStorage.getItem('skill-deck-layout')!;
    useLayoutStore.setState({ skillsListWidth: null, libraryListWidth: null });
    localStorage.setItem('skill-deck-layout', stored);
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBe(400);
    expect(useLayoutStore.getState().libraryListWidth).toBe(280);
  });

  it('keeps the untouched workspace automatic after rehydrating a manual resize', async () => {
    useLayoutStore.getState().setSkillsListWidth(400);
    const stored = localStorage.getItem('skill-deck-layout')!;
    useLayoutStore.setState({ skillsListWidth: null, libraryListWidth: null });
    localStorage.setItem('skill-deck-layout', stored);
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBe(400);
    expect(useLayoutStore.getState().libraryListWidth).toBeNull();
  });

  it.each([null, { skillsListWidth: -100 }, { skillsListWidth: '400' }])('ignores invalid stored preferences: %j', async (state) => {
    localStorage.setItem('skill-deck-layout', JSON.stringify({ state, version: 2 }));
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState().skillsListWidth).toBeNull();
  });

  it('persists sidebar widths independently without changing the list preferences', async () => {
    useLayoutStore.getState().setSkillsSidebarWidth(296.4);
    useLayoutStore.getState().setLibrarySidebarWidth(224);
    const stored = localStorage.getItem('skill-deck-layout')!;
    useLayoutStore.setState({ skillsSidebarWidth: 208, librarySidebarWidth: 208 });
    localStorage.setItem('skill-deck-layout', stored);
    await useLayoutStore.persist.rehydrate();
    expect(useLayoutStore.getState()).toMatchObject({
      skillsSidebarWidth: 296, librarySidebarWidth: 224,
      skillsListWidth: null, libraryListWidth: null,
    });
    for (const width of [NaN, Infinity, 191, 321]) {
      useLayoutStore.getState().setSkillsSidebarWidth(width);
      expect(useLayoutStore.getState().skillsSidebarWidth).toBe(296);
    }
  });
});
