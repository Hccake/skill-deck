/* @vitest-environment jsdom */

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { InstallResponse, MutationUnitResult } from '@/bindings';
import type { WizardState } from '../types';
import { CompleteStep } from '../CompleteStep';

vi.mock('@/components/recovery/RecoveryActions', () => ({
  RecoveryActions: ({ recovery }: { recovery: { resourceId: string } }) => <div>recovery-actions:{recovery.resourceId}</div>,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options ? `${key}:${JSON.stringify(options)}` : key,
  }),
}));

vi.mock('@/utils/cross-storage-guidance', () => ({
  getCrossStorageFailureGuidance: () => 'crossStorage.failureGuidance',
}));

function unit(status: MutationUnitResult['status'], overrides = {}): MutationUnitResult {
  return {
    unitId: 'demo',
    skillName: 'Demo',
    source: null,
    target: { environment: { kind: 'native' }, scope: { scope: 'global' } },
    status,
    retryable: status !== 'succeeded',
    lockCommitted: status === 'succeeded',
    actualMode: status === 'succeeded' ? 'symlink' : null,
    fallbackReason: null,
    agentTargets: [],
    warnings: [],
    error: status === 'succeeded' ? null : {
      code: 'executionFailed',
      parameters: {},
      field: null,
      severity: 'error',
      retryable: true,
      technicalDetails: 'permission denied',
      environment: null,
      context: null,
      unitId: 'demo',
      recoveryResourceId: null,
      displayPaths: [],
    },
    recovery: null,
    ...overrides,
  };
}

function state(response: InstallResponse): WizardState {
  return {
    step: 'complete',
    entryPoint: 'skills-panel',
    scope: 'global',
    context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
    source: 'owner/repo',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [],
    selectedSkills: [],
    skillFilter: null,
    skillSearchQuery: '',
    overwrites: {},
    preparation: { status: 'idle' },
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: response,
  };
}

describe('CompleteStep', () => {
  it('renders authoritative succeeded and failed mutation units', () => {
    render(
      <CompleteStep
        state={state({ warnings: [], units: [
          unit('succeeded'),
          unit('failed', { unitId: 'install:broken', skillName: 'Broken' }),
        ] })}
      />,
    );

    expect(screen.getByText('Demo')).toBeDefined();
    expect(screen.getByText('Broken')).toBeDefined();
    expect(screen.queryByText('install:broken')).toBeNull();
    expect(screen.getByText('addSkill.complete.partial')).toBeDefined();
  });

  it('shows a structured warning when suppression cleanup fails after installation', () => {
    render(
      <CompleteStep
        state={state({
          units: [unit('succeeded')],
          warnings: ['suppressionCleanupFailed'],
        } as unknown as InstallResponse)}
      />,
    );

    expect(screen.getByText('addSkill.complete.warnings.suppressionCleanupFailed')).toBeDefined();
  });

  it('leaves wizard actions to the page footer', () => {
    render(
      <CompleteStep
        state={state({ warnings: [], units: [unit('failed')] })}
      />,
    );

    expect(screen.queryByRole('button', { name: 'addSkill.actions.retry' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'addSkill.actions.done' })).toBeNull();
  });

  it('shows recovery-required units distinctly', () => {
    render(
      <CompleteStep
        state={state({ warnings: [], units: [unit('recoveryRequired', {
          recovery: { resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' },
        })] })}
      />,
    );

    expect(screen.getByText('addSkill.complete.recoveryRequired')).toBeDefined();
    expect(screen.getByText('recovery-actions:recovery-1')).toBeDefined();
  });

  it('keeps technical diagnostics behind disclosure and omits ordinary recovery retry', () => {
    render(
      <CompleteStep
        state={state({ warnings: [], units: [unit('recoveryRequired', {
          retryable: true,
          recovery: { resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' },
        })] })}
      />,
    );

    expect(screen.queryByText('permission denied')).toBeNull();
    expect(screen.getByText('mutation.result.errors.executionFailed')).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'addSkill.complete.errorDetails' }));
    expect(screen.getByText('demo: permission denied')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'addSkill.actions.retry' })).toBeNull();
  });
});
