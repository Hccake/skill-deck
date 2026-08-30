import { describe, expect, it } from 'vitest';
import type { AppError } from '@/bindings';
import { formatAppError } from '../format-app-error';

const t = (key: string, params?: Record<string, unknown>) =>
  `${key}${params ? JSON.stringify(params) : ''}`;

describe('formatAppError', () => {
  it('formats a Skill placement conflict with its target path', () => {
    expect(formatAppError({
      kind: 'skillPlacementTargetConflict',
      data: {
        skillName: 'demo',
        agentIds: ['agent-demo'],
        targetPath: '/agent/skills/demo',
        targetKind: 'file',
      },
    }, t as never)).toBe(
      'mutation.result.errors.skillPlacementTargetConflict{"skillName":"demo","targetPath":"/agent/skills/demo","targetKind":"mutation.result.targetKinds.file"}',
    );
  });

  it('does not report a generic Git failure as a network error', () => {
    const error = {
      kind: 'gitCloneFailed',
      data: { message: 'git exited with status 128' },
    } as AppError;

    expect(formatAppError(error, t as never)).toBe(
      'addSkill.source.error.gitFailed{"details":"git exited with status 128"}'
    );
  });

  it('bounds Git diagnostics rendered in the source step', () => {
    const error = {
      kind: 'gitCloneFailed',
      data: { message: `prefix-${'x'.repeat(4_000)}-suffix` },
    } as AppError;

    const rendered = formatAppError(error, t as never);

    expect(rendered.length).toBeLessThan(2_100);
    expect(rendered).toContain('prefix-');
    expect(rendered).toContain('-suffix');
  });

  it('renders direct-download failures from stable localized reason codes', () => {
    expect(formatAppError({
      kind: 'sourceAcquisitionFailed',
      data: { wellKnownReason: 'notFound', downloadReason: 'limitExceeded' },
    }, t as never)).toBe(
      'addSkill.source.error.acquisitionFailed{"wellKnownReason":"addSkill.source.error.acquisitionReason.notFound","downloadReason":"addSkill.source.error.acquisitionReason.limitExceeded"}'
    );
    expect(formatAppError({
      kind: 'directDownloadFailed',
      data: { reason: 'unsafeArchive' },
    }, t as never)).toBe('addSkill.source.error.downloadFailure.unsafeArchive');
  });

  it('renders a scoped Well-known mismatch as its own public error', () => {
    const error = {
      kind: 'wellKnownScopeNotFound',
      data: { scopePath: '/collections/team', rootUrl: 'https://example.com' },
    } as unknown as AppError;

    expect(formatAppError(error, t as never)).toBe(
      'addSkill.source.error.scopeNotFound{"scopePath":"/collections/team","rootUrl":"https://example.com"}'
    );
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
        data: { target: { kind: 'rootField', field: 'defaultTargetAgents' } },
      },
      'addSkill.error.agentDefaultsConflict',
    ],
  ])('formats structured lock conflict target', (error, expected) => {
    expect(formatAppError(error as unknown as AppError, t as never)).toBe(expected);
  });

  it.each<[AppError, string]>([
    [{ kind: 'applicationTerminating' }, 'addSkill.error.applicationTerminating'],
    [
      { kind: 'installWizardActive' } as AppError,
      'addSkill.error.installWizardActive',
    ],
    [
      { kind: 'installWizardSessionUnavailable' } as unknown as AppError,
      'addSkill.error.installWizardSessionUnavailable',
    ],
    [{ kind: 'wslIntegrationBusy', data: { reason: 'mutation' } }, 'settings.general.wslBusyMutation'],
    [{ kind: 'wslIntegrationBusy', data: { reason: 'lifecycle' } }, 'settings.general.wslBusyLifecycle'],
    [
      { kind: 'wslIntegrationBusy', data: { reason: 'installWizard' } },
      'settings.general.wslBusyInstallWizard',
    ],
    [
      { kind: 'wslIntegrationBusy', data: { reason: 'wslOperation' } },
      'settings.general.wslBusyOperation',
    ],
    [{ kind: 'mutationCancelled' }, 'addSkill.error.mutationCancelled'],
    [
      { kind: 'environmentDiscoveryFailed', data: { message: 'wsl list failed' } },
      'addSkill.error.environmentDiscoveryFailed',
    ],
    [{ kind: 'wslCommandTimedOut' }, 'addSkill.error.wslCommandTimedOut'],
    [
      { kind: 'wslOutputLimitExceeded', data: { stream: 'stdout', limit: 1024 } },
      'addSkill.error.wslOutputLimitExceeded{"stream":"stdout","limit":1024}',
    ],
    [
      { kind: 'wslCommandFailed', data: { exitCode: 7, stderr: 'command failed' } },
      'addSkill.error.wslCommandFailed{"exitCode":7}',
    ],
    [
      {
        kind: 'environmentUnavailable',
        data: {
          environment: { kind: 'wsl', distro_name: 'Ubuntu' },
          message: 'session expired',
        },
      },
      'addSkill.error.environmentUnavailable',
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
    ],
    [{ kind: 'configurationReadOnly' }, 'addSkill.error.configurationReadOnly'],
    [
      { kind: 'payloadStorageRequiresCleanup', data: { environment: { kind: 'native' } } },
      'addSkill.error.payloadStorageRequiresCleanup',
    ],
    [
      { kind: 'validation', data: { field: 'request', message: 'invalid selection' } },
      'invalid selection',
    ],
    [
      { kind: 'agentSelectionInvalid', data: { reason: 'placementConflict' } } as AppError,
      'agentSelection.error.placementConflict',
    ],
    [{ kind: 'staleContext' }, 'addSkill.error.staleState'],
    [
      {
        kind: 'recoveryRequired',
        data: { recovery_resource_id: 'recovery-1', message: 'manual recovery required' },
      },
      'manual recovery required',
    ],
  ])('formats $kind without falling back to the unknown error', (error, expected) => {
    expect(formatAppError(error, t as never)).toBe(expected);
  });
});
