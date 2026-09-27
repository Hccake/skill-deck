import { describe, expect, it } from 'vitest';
import { resolveSkillsLayout } from '../skills-layout';

describe('Skills layout constraints', () => {
  it('gives spare space to detail and keeps the preferred list width', () => {
    expect(resolveSkillsLayout(1120, true, 320, false)).toEqual({
      sidebarWidth: 208, listWidth: 320, navigationCollapsed: false,
    });
    expect(resolveSkillsLayout(1440, true, 420, false).listWidth).toBe(420);
  });

  it('shrinks the list before the navigation and protects the detail minimum', () => {
    expect(resolveSkillsLayout(840, true, 420, false)).toEqual({
      sidebarWidth: 208, listWidth: 271, navigationCollapsed: false,
    });
    expect(resolveSkillsLayout(800, true, 420, false)).toEqual({
      sidebarWidth: 199, listWidth: 240, navigationCollapsed: false,
    });
  });

  it('uses temporary navigation only when all three columns cannot fit', () => {
    expect(resolveSkillsLayout(720, false, 320, false).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(720, true, 320, false)).toEqual({
      sidebarWidth: 0, listWidth: 320, navigationCollapsed: true,
    });
    expect(resolveSkillsLayout(794, true, 320, false).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(800, true, 320, true).navigationCollapsed).toBe(true);
    expect(resolveSkillsLayout(818, true, 320, true).navigationCollapsed).toBe(false);
    expect(resolveSkillsLayout(720, false, 320, true).navigationCollapsed).toBe(false);
  });
});
