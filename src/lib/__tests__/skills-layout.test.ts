import { describe, expect, it } from 'vitest';
import { resolveSkillsLayout } from '../skills-layout';

describe('Skills layout constraints', () => {
  it.each([
    [720, 0, 240],
    [800, 0, 240],
    [900, 0, 269.7],
    [1000, 208, 240],
    [1280, 208, 320],
  ])('gives detail the remaining space with automatic widths at %ipx', (width, sidebarWidth, listWidth) => {
    const layout = resolveSkillsLayout(width, true, null, false);
    expect(layout.sidebarWidth).toBe(sidebarWidth);
    expect(layout.listWidth).toBeCloseTo(listWidth);
    expect(layout.navigationCollapsed).toBe(sidebarWidth === 0);
    expect(width - layout.sidebarWidth - layout.listWidth - 1).toBeGreaterThanOrEqual(479);
  });

  it('gives spare space to detail and keeps the preferred list width', () => {
    expect(resolveSkillsLayout(1120, true, 320, false)).toMatchObject({
      sidebarWidth: 208, listWidth: 320, navigationCollapsed: false,
    });
    expect(resolveSkillsLayout(1440, true, 420, false).listWidth).toBe(420);
  });

  it('preserves manual widths and protects the detail minimum when the window narrows', () => {
    expect(resolveSkillsLayout(840, true, 420, false)).toMatchObject({
      sidebarWidth: 0, listWidth: 420, navigationCollapsed: true,
    });
    expect(resolveSkillsLayout(720, true, 420, false)).toMatchObject({
      sidebarWidth: 0, listWidth: 359, navigationCollapsed: true,
    });
    expect(resolveSkillsLayout(1280, true, 420, true).listWidth).toBe(420);
  });

  it('restores three columns only when the default detail has comfortable reading space', () => {
    expect(resolveSkillsLayout(720, false, 320, false).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(720, true, 320, false)).toMatchObject({
      sidebarWidth: 0, listWidth: 320, navigationCollapsed: true,
    });
    expect(resolveSkillsLayout(928, true, null, false).navigationCollapsed).toBe(true);
    expect(resolveSkillsLayout(929, true, null, false).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(952, true, null, true).navigationCollapsed).toBe(true);
    expect(resolveSkillsLayout(953, true, null, true).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(720, false, 320, true).navigationCollapsed).toBe(false);
  });

  it('keeps each chosen sidebar width until the window needs temporary navigation', () => {
    expect(resolveSkillsLayout(1280, true, null, false, 300).sidebarWidth).toBe(300);
    expect(resolveSkillsLayout(1000, true, null, false, 300).navigationCollapsed).toBe(true);
    expect(resolveSkillsLayout(1280, true, null, true, 300).sidebarWidth).toBe(300);
    expect(resolveSkillsLayout(720, false, null, false, 300).sidebarWidth).toBe(300);
  });

  it('bounds sidebar resizing without taking the default detail reading space', () => {
    expect(resolveSkillsLayout(1280, true, null, false).sidebarMaxWidth).toBe(320);
    expect(resolveSkillsLayout(1000, true, null, false).sidebarMaxWidth).toBe(279);
    expect(resolveSkillsLayout(1000, true, null, false, 279)).toMatchObject({
      sidebarWidth: 279, navigationCollapsed: false, listWidth: 240,
    });
    expect(resolveSkillsLayout(720, false, null, false).sidebarMaxWidth).toBe(320);
  });
});
