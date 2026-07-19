import { describe, expect, it } from 'vitest';
import type { CustomScopeDefinition, ScopeDefinition } from '@/bindings';
import {
  formatPathRule,
  scopeReadMode,
} from '../agent-settings-presentation';

describe('agent settings presentation', () => {
  it('maps built-in scope definitions to the four read modes', () => {
    const unsupported: ScopeDefinition = {
      enabled: false,
      readsShared: false,
      privatePath: null,
    };
    const shared: ScopeDefinition = {
      enabled: true,
      readsShared: true,
      privatePath: null,
    };
    const privateOnly: ScopeDefinition = {
      enabled: true,
      readsShared: false,
      privatePath: { kind: 'project', relativePath: '.agent/skills' },
    };
    const both: ScopeDefinition = {
      enabled: true,
      readsShared: true,
      privatePath: { kind: 'home', relativePath: '.agent/skills' },
    };

    expect(scopeReadMode(unsupported)).toBe('unsupported');
    expect(scopeReadMode(shared)).toBe('shared');
    expect(scopeReadMode(privateOnly)).toBe('private');
    expect(scopeReadMode(both)).toBe('both');
  });

  it('maps custom scope definitions without treating the scope as a status', () => {
    const scope = (enabled: boolean, location: CustomScopeDefinition['location']): CustomScopeDefinition => ({
      enabled,
      location,
      privatePath: location === 'shared'
        ? null
        : { kind: 'based', base: 'home', relativePath: '.agent/skills' },
    });

    expect(scopeReadMode(scope(false, 'both'))).toBe('unsupported');
    expect(scopeReadMode(scope(true, 'shared'))).toBe('shared');
    expect(scopeReadMode(scope(true, 'private'))).toBe('private');
    expect(scopeReadMode(scope(true, 'both'))).toBe('both');
  });

  it('formats stable path rules without resolving paths', () => {
    expect(formatPathRule({ kind: 'home', relativePath: '.agent/skills' }))
      .toBe('Home / .agent/skills');
    expect(formatPathRule({
      kind: 'based',
      base: 'configHome',
      relativePath: 'agent/skills',
    })).toBe('ConfigHome / agent/skills');
    expect(formatPathRule({ kind: 'absolute', path: '/opt/agent/skills' }))
      .toBe('/opt/agent/skills');
  });

  it('includes environment-variable and first-existing fallbacks in searchable path rules', () => {
    expect(formatPathRule({
      kind: 'environmentVariable',
      name: 'CODEX_HOME',
      relativePath: '',
      fallback: {
        kind: 'firstExisting',
        candidates: [{ kind: 'home', relativePath: '.codex' }],
        fallback: { kind: 'configHome', relativePath: 'codex' },
      },
    })).toContain('ConfigHome / codex');
  });
});
