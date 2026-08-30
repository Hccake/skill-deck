import { describe, expect, it } from 'vitest';
import type { AppError } from '@/bindings';
import { parseInstallError } from '../parse-install-error';

const t = (key: string, params?: Record<string, unknown>) =>
  `${key}${params ? JSON.stringify(params) : ''}`;

describe('parseInstallError', () => {
  it('keeps a Skill placement conflict actionable if it crosses the shared formatter', () => {
    const result = parseInstallError({
      kind: 'skillPlacementTargetConflict',
      data: {
        skillName: 'demo',
        agentIds: ['agent-demo'],
        targetPath: '/agent/skills/demo',
        targetKind: 'file',
      },
    }, t as never);

    expect(result.message).toBe(
      'mutation.result.errors.skillPlacementTargetConflict{"skillName":"demo","targetPath":"/agent/skills/demo","targetKind":"mutation.result.targetKinds.file"}',
    );
  });

  it('preserves generic Git failure details without calling it a network failure', () => {
    const error = {
      kind: 'gitCloneFailed',
      data: { message: 'git exited with status 128' },
    } as AppError;

    const result = parseInstallError(error, t as never);

    expect(result.message).toBe('addSkill.error.gitFailed');
    expect(result.details).toBe('git exited with status 128');
    expect(result.suggestions).not.toContain('addSkill.error.suggestion.checkNetwork');
  });

  it.each([
    [
      {
        kind: 'lockConflict',
        data: { target: { kind: 'skill', skillName: 'toolkit' } },
      },
      'addSkill.error.lockConflict{"skill":"toolkit"}',
    ],
    [
      {
        kind: 'lockConflict',
        data: { target: { kind: 'rootField', field: 'lastSelectedAgents' } },
      },
      'addSkill.error.agentDefaultsConflict',
    ],
  ])('parses structured lock conflict target', (error, expected) => {
    const result = parseInstallError(error as unknown as AppError, t as never);

    expect(result.message).toBe(expected);
  });

  it('includes configured timeout details and settings guidance for git timeouts', () => {
    const error = {
      kind: 'gitTimeout',
      data: { timeoutSecs: 300 },
    } as AppError & { data: { timeoutSecs: number } };

    const result = parseInstallError(
      error,
      t as never
    );

    expect(result.message).toContain('addSkill.error.cloneTimeout');
    expect(result.details).toContain('300');
    expect(result.suggestions).toContain('addSkill.error.suggestion.adjustCloneTimeout');
  });

  it('uses stable direct-download reason codes without exposing backend details', () => {
    const result = parseInstallError({
      kind: 'directDownloadFailed',
      data: { reason: 'archiveTooLarge' },
    }, t as never);

    expect(result).toEqual({
      message: 'addSkill.source.error.downloadFailure.archiveTooLarge',
    });
  });

  it('keeps a scoped Well-known mismatch distinct from a missing download', () => {
    const result = parseInstallError({
      kind: 'wellKnownScopeNotFound',
      data: { scopePath: '/collections/team', rootUrl: 'https://example.com' },
    } as unknown as AppError, t as never);

    expect(result).toEqual({
      message: 'addSkill.source.error.scopeNotFound{"scopePath":"/collections/team","rootUrl":"https://example.com"}',
      suggestions: ['addSkill.error.suggestion.checkSource'],
    });
  });

  it.each<[AppError, string, string | undefined]>([
    [
      { kind: 'applicationTerminating' },
      'addSkill.error.applicationTerminating',
      undefined,
    ],
    [
      { kind: 'installWizardActive' },
      'addSkill.error.installWizardActive',
      undefined,
    ],
    [
      { kind: 'installWizardSessionUnavailable' } as unknown as AppError,
      'addSkill.error.installWizardSessionUnavailable',
      undefined,
    ],
    [
      { kind: 'mutationCancelled' },
      'addSkill.error.mutationCancelled',
      undefined,
    ],
    [
      { kind: 'environmentDiscoveryFailed', data: { message: 'wsl list failed' } },
      'addSkill.error.environmentDiscoveryFailed',
      'wsl list failed',
    ],
    [
      { kind: 'wslCommandTimedOut' },
      'addSkill.error.wslCommandTimedOut',
      undefined,
    ],
    [
      { kind: 'wslOutputLimitExceeded', data: { stream: 'stderr', limit: 2048 } },
      'addSkill.error.wslOutputLimitExceeded{"stream":"stderr","limit":2048}',
      undefined,
    ],
    [
      { kind: 'wslCommandFailed', data: { exitCode: 23, stderr: 'permission denied' } },
      'addSkill.error.wslCommandFailed{"exitCode":23}',
      'permission denied',
    ],
    [
      {
        kind: 'environmentUnavailable',
        data: {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          message: 'distribution stopped',
        },
      },
      'addSkill.error.environmentUnavailable',
      'distribution stopped',
    ],
    [
      {
        kind: 'storageMappingUnsupported',
        data: {
          path: '\\\\server\\share',
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
        },
      },
      'addSkill.error.storageMappingUnsupported{"path":"\\\\\\\\server\\\\share"}',
      undefined,
    ],
    [
      { kind: 'configurationReadOnly' },
      'addSkill.error.configurationReadOnly',
      undefined,
    ],
    [
      { kind: 'payloadStorageRequiresCleanup', data: { environment: { kind: 'native' } } },
      'addSkill.error.payloadStorageRequiresCleanup',
      undefined,
    ],
    [
      { kind: 'validation', data: { field: 'request', message: 'invalid selection' } },
      'invalid selection',
      undefined,
    ],
    [
      { kind: 'stalePayload' },
      'addSkill.error.staleState',
      undefined,
    ],
    [
      {
        kind: 'recoveryRequired',
        data: { recovery_resource_id: 'recovery-1', message: 'manual recovery required' },
      },
      'manual recovery required',
      undefined,
    ],
  ])('preserves actionable details for $kind', (error, message, details) => {
    const result = parseInstallError(error, t as never);

    expect(result.message).toBe(message);
    expect(result.details).toBe(details);
  });

  it('guides users to an environment with writable agent configuration', () => {
    const result = parseInstallError({ kind: 'configurationReadOnly' }, t as never);

    expect(result.suggestions).toEqual([
      'addSkill.error.suggestion.chooseWritableConfiguration',
    ]);
  });
});
