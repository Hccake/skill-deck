import { describe, expect, it } from 'vitest';
import {
  createAgentDraft,
  updateAgentDraft,
} from '../agent-definition-draft';

describe('agent definition draft', () => {
  it('derives the ID and default private paths from the display name', () => {
    const current = createAgentDraft();
    const next = updateAgentDraft(current, { ...current, displayName: 'Foo Code' });

    expect(next.id).toBe('foo-code');
    expect(next.global.privatePath).toEqual({
      kind: 'based', base: 'home', relativePath: '.foo-code/skills',
    });
    expect(next.project.privatePath).toEqual({
      kind: 'based', base: 'project', relativePath: '.foo-code/skills',
    });
    expect(next.detectionPaths).toEqual([
      { kind: 'based', base: 'home', relativePath: '.foo-code' },
    ]);
  });

  it('keeps a user-edited path when the ID changes', () => {
    const current = createAgentDraft('foo-code', 'Foo Code');
    const customized = {
      ...current,
      global: {
        ...current.global,
        privatePath: { kind: 'based' as const, base: 'home' as const, relativePath: '.custom/skills' },
      },
    };
    const next = updateAgentDraft(customized, { ...customized, id: 'bar-code' });

    expect(next.global.privatePath).toEqual(customized.global.privatePath);
    expect(next.project.privatePath).toEqual({
      kind: 'based', base: 'project', relativePath: '.bar-code/skills',
    });
    expect(next.detectionPaths[0]).toEqual({
      kind: 'based', base: 'home', relativePath: '.bar-code',
    });
  });

  it('stops deriving the ID after the user edits it', () => {
    const current = createAgentDraft('foo-code', 'Foo Code');
    const editedId = updateAgentDraft(current, { ...current, id: 'my-agent' });
    const renamed = updateAgentDraft(editedId, { ...editedId, displayName: 'Bar Code' });

    expect(renamed.id).toBe('my-agent');
  });

  it('keeps a path detached after the user changes it and later restores its old text', () => {
    const detached = new Set<string>();
    const current = createAgentDraft('foo-code', 'Foo Code');
    const customized = updateAgentDraft(current, {
      ...current,
      global: {
        ...current.global,
        privatePath: { kind: 'based', base: 'home', relativePath: '.custom/skills' },
      },
    }, detached);
    const restored = updateAgentDraft(customized, {
      ...customized,
      global: { ...customized.global, privatePath: current.global.privatePath },
    }, detached);
    const renamed = updateAgentDraft(restored, { ...restored, id: 'bar-code' }, detached);

    expect(renamed.global.privatePath).toEqual(current.global.privatePath);
    expect(renamed.project.privatePath).toEqual({
      kind: 'based', base: 'project', relativePath: '.bar-code/skills',
    });
  });
});
