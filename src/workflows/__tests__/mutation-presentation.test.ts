import { describe, expect, it } from 'vitest';
import type { MutationUnitResult } from '@/bindings';
import { presentMutationResults, presentMutationUnit } from '../mutation-presentation';

const t = (key: string, parameters?: Partial<Record<string, string>>) => (
  `${key}${parameters ? JSON.stringify(parameters) : ''}`
);

function failedUnit(): MutationUnitResult {
  return {
    unitId: 'skill-a',
    skillName: 'Skill A',
    source: null,
    target: { environment: { kind: 'native' }, scope: { scope: 'global' } },
    status: 'failed',
    retryable: true,
    lockCommitted: false,
    actualMode: null,
    fallbackReason: null,
    agentTargets: [],
    warnings: [{
      code: 'backupCleanupFailed',
      parameters: {},
      technicalDetails: 'raw warning detail',
    }],
    error: {
      code: 'unsafePath',
      parameters: { path: '/secret' },
      field: null,
      severity: 'error',
      retryable: true,
      technicalDetails: 'permission denied at /secret',
      environment: { kind: 'native' },
      context: null,
      unitId: 'skill-a',
      recoveryResourceId: null,
      displayPaths: [],
    },
    recovery: null,
  };
}

describe('mutation result presentation', () => {
  it('uses stable codes for primary copy and keeps technical details diagnostic-only', () => {
    const presentation = presentMutationResults([failedUnit()], t);

    expect(presentation.failedUnits).toEqual([{
      unitId: 'skill-a',
      skillName: 'Skill A',
      message: 'mutation.result.errors.unsafePath{"path":"/secret"}',
    }]);
    expect(presentation.warnings).toEqual(['mutation.result.warnings.backupCleanupFailed']);
    expect(presentation.summary).not.toContain('permission denied');
    expect(presentation.diagnostics).toEqual([
      'skill-a: permission denied at /secret',
      'skill-a: raw warning detail',
    ]);
    expect(presentation.summary).toContain('Skill A:');
    expect(presentation.summary).not.toContain('skill-a:');
  });

  it('resolves user-facing environment and project labels without parsing unitId', () => {
    const unit = failedUnit();
    unit.unitId = 'copy:internal-id:opaque-project-id';
    unit.target = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu-24.04' },
      scope: { scope: 'project', project_id: 'project-1' },
    };

    const presentation = presentMutationUnit(unit, t, {
      environments: [{
        environment: { kind: 'wsl', distro_name: 'Ubuntu-24.04' },
        displayName: 'Ubuntu',
        status: 'available',
        revision: 1,
        error: null,
      }],
      projectsByEnvironment: {
        'wsl:ubuntu-24.04': [{
          binding: {
            id: 'project-1',
            nativePath: '/home/alice/project',
            displayName: 'Skill Deck',
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: { access: 'native', owner: null },
        }],
      },
    });

    expect(presentation.skillName).toBe('Skill A');
    expect(presentation.environmentLabel)
      .toBe('context.environmentWslName{"environment":"Ubuntu"}');
    expect(presentation.scopeLabel).toBe('Skill Deck');
  });
});
