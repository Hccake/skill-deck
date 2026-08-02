import { describe, expect, it } from 'vitest';
import type { AppError } from '@/bindings';
import { formatAppError } from '../format-app-error';

const t = (key: string, params?: Record<string, unknown>) =>
  `${key}${params ? JSON.stringify(params) : ''}`;

describe('formatAppError', () => {
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
      { kind: 'payloadStorageRequiresCleanup', data: { environment: { kind: 'host' } } },
      'addSkill.error.payloadStorageRequiresCleanup',
    ],
    [
      { kind: 'validation', data: { field: 'request', message: 'invalid selection' } },
      'invalid selection',
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
