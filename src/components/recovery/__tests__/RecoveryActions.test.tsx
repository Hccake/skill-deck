/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RecoveryActions } from '../RecoveryActions';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';

const mocks = vi.hoisted(() => ({
  getStatus: vi.fn(),
  open: vi.fn(),
  confirm: vi.fn(),
  getInstallWizardSession: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getRecoveryResourceStatus: (id: string) => mocks.getStatus(id),
  openRecoveryResource: (id: string) => mocks.open(id),
  confirmRecoveryResourceResolved: (id: string, revision: string) => mocks.confirm(id, revision),
  getInstallWizardSession: () => mocks.getInstallWizardSession(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) => (
      values ? `${key}:${JSON.stringify(values)}` : key
    ),
    i18n: { language: 'zh-CN' },
  }),
}));

describe('RecoveryActions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.open.mockResolvedValue(undefined);
    mocks.confirm.mockResolvedValue(undefined);
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      hasConfirmedSnapshot: false,
      syncError: null,
      monitorRetryRevision: 0,
      snapshotVersion: 0,
    });
    mocks.getInstallWizardSession.mockResolvedValue({ revision: 1, active: true });
  });

  it('opens opaque recovery data and confirms cleanup with the displayed revision', async () => {
    mocks.getStatus.mockResolvedValue({
      resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-1',
      environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
    });
    const onResolved = vi.fn();
    render(<RecoveryActions recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }} onResolved={onResolved} />);

    await screen.findByText('recovery.state.consistentCanCleanup');
    fireEvent.click(screen.getByRole('button', { name: 'recovery.openDirectory' }));
    expect(mocks.open).toHaveBeenCalledWith('recovery-1');

    fireEvent.click(screen.getByRole('button', { name: 'recovery.cleanup' }));
    fireEvent.click(screen.getByRole('button', { name: 'recovery.confirmCleanup' }));
    await waitFor(() => expect(mocks.confirm).toHaveBeenCalledWith('recovery-1', 'revision-1'));
    expect(onResolved).toHaveBeenCalled();
  });

  it('never offers cleanup while recovery still needs attention', async () => {
    mocks.getStatus.mockResolvedValue({
      resourceId: 'recovery-1', state: 'needsAttention', revision: 'revision-1',
      environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
    });
    render(<RecoveryActions recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }} />);

    await screen.findByText('recovery.state.needsAttention');
    expect(screen.queryByRole('button', { name: 'recovery.cleanup' })).toBeNull();
  });

  it('reuses an enumerated status without issuing an N+1 status request', () => {
    render(<RecoveryActions
      recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }}
      initialStatus={{
        resourceId: 'recovery-1', state: 'needsAttention', revision: 'revision-1',
        environment: { kind: 'native' }, createdAtEpochMs: 1, subject: null, paths: [], diagnostic: null,
      }}
    />);

    expect(screen.getByText('recovery.state.needsAttention')).toBeDefined();
    expect(mocks.getStatus).not.toHaveBeenCalled();
  });

  it('identifies an interrupted update and labels the current and backup paths', () => {
    render(<RecoveryActions
      recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }}
      initialStatus={{
        resourceId: 'recovery-1',
        state: 'needsAttention',
        revision: 'revision-1',
        environment: { kind: 'native' },
        createdAtEpochMs: 1_786_080_000_000,
        subject: {
          operationKind: 'update',
          skillName: 'skill-deck',
          context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
        },
        paths: [
          {
            kind: 'current',
            location: {
              environment: { kind: 'native' },
              nativePath: 'C:\\Users\\cheng\\.agents\\skills\\skill-deck',
            },
          },
          {
            kind: 'backup',
            location: {
              environment: { kind: 'native' },
              nativePath: 'C:\\Users\\cheng\\.agents\\skills\\.skill-deck-backup-update',
            },
          },
        ],
        diagnostic: null,
      }}
    />);

    expect(screen.getByRole('heading', {
      name: 'recovery.itemTitle.update:{"skillName":"skill-deck"}',
    })).toBeDefined();
    expect(screen.getByText('recovery.path.current')).toBeDefined();
    expect(screen.getByText('C:\\Users\\cheng\\.agents\\skills\\skill-deck')).toBeDefined();
    expect(screen.getByText('recovery.path.backup')).toBeDefined();
    expect(screen.getByText('C:\\Users\\cheng\\.agents\\skills\\.skill-deck-backup-update'))
      .toBeDefined();
    expect(screen.getByRole('button', { name: 'recovery.openDirectory' })).toBeDefined();
    expect(document.querySelector('[aria-live="assertive"]')).toBeNull();
    expect(screen.getByText('recovery.state.needsAttention').parentElement?.getAttribute('aria-live'))
      .toBe('polite');
  });

  it('identifies an interrupted source repair by its original operation', () => {
    render(<RecoveryActions
      recovery={{ resourceId: 'recovery-repair', suggestedActionCode: 'reviewChanges' }}
      initialStatus={{
        resourceId: 'recovery-repair',
        state: 'needsAttention',
        revision: 'revision-repair',
        environment: { kind: 'native' },
        createdAtEpochMs: 1_786_080_000_000,
        subject: {
          operationKind: 'repair',
          skillName: 'skill-deck',
          context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
        },
        paths: [],
        diagnostic: null,
      }}
    />);

    expect(screen.getByRole('heading', {
      name: 'recovery.itemTitle.repair:{"skillName":"skill-deck"}',
    })).toBeDefined();
  });

  it('keeps the recovery action visible and refreshes status when cleanup fails', async () => {
    mocks.getStatus
      .mockResolvedValueOnce({
        resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-1',
        environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
      })
      .mockResolvedValueOnce({
        resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-2',
        environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
      });
    mocks.confirm.mockRejectedValue(new Error('cleanup failed'));
    const onResolved = vi.fn();
    render(<RecoveryActions recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }} onResolved={onResolved} />);

    await screen.findByText('recovery.state.consistentCanCleanup');
    fireEvent.click(screen.getByRole('button', { name: 'recovery.cleanup' }));
    fireEvent.click(screen.getByRole('button', { name: 'recovery.confirmCleanup' }));

    await waitFor(() => expect(mocks.getStatus).toHaveBeenCalledTimes(2));
    expect(onResolved).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'recovery.cleanup' })).toBeDefined();
  });

  it('keeps cleanup local feedback clear when the install flow wins the race', async () => {
    mocks.getStatus.mockResolvedValue({
      resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-1',
      environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
    });
    mocks.confirm.mockRejectedValue({ kind: 'installWizardActive' });
    const onResolved = vi.fn();
    render(<RecoveryActions
      recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }}
      onResolved={onResolved}
    />);

    await screen.findByText('recovery.state.consistentCanCleanup');
    fireEvent.click(screen.getByRole('button', { name: 'recovery.cleanup' }));
    fireEvent.click(screen.getByRole('button', { name: 'recovery.confirmCleanup' }));

    await waitFor(() => expect(useInstallWizardSessionStore.getState().active).toBe(true));
    expect(onResolved).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect((screen.getByRole('button', { name: 'recovery.cleanup' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });

  it('reports an open failure instead of leaving an unhandled rejection', async () => {
    mocks.getStatus.mockResolvedValue({
      resourceId: 'recovery-1', state: 'invalid', revision: '',
      environment: { kind: 'native' }, subject: null, paths: [{
        kind: 'record', location: { environment: { kind: 'native' }, nativePath: '/tmp/recovery' },
      }],
      diagnostic: null,
    });
    mocks.open.mockRejectedValue(new Error('open failed'));

    render(<RecoveryActions recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }} />);
    await screen.findByText('recovery.state.invalid');
    fireEvent.click(screen.getByRole('button', { name: 'recovery.openRecordDirectory' }));

    await waitFor(() => expect(screen.getByText('recovery.openError')).toBeDefined());
  });

  it('keeps recovery inspection available but blocks cleanup during the wizard session', async () => {
    useInstallWizardSessionStore.setState({ revision: 1, active: true });
    mocks.getStatus.mockResolvedValue({
      resourceId: 'recovery-1', state: 'consistentCanCleanup', revision: 'revision-1',
      environment: { kind: 'native' }, subject: null, paths: [], diagnostic: null,
    });

    render(<RecoveryActions recovery={{ resourceId: 'recovery-1', suggestedActionCode: 'reviewChanges' }} />);
    await screen.findByText('recovery.state.consistentCanCleanup');

    expect((screen.getByRole('button', { name: 'recovery.openDirectory' }) as HTMLButtonElement).disabled)
      .toBe(false);
    expect((screen.getByRole('button', { name: 'recovery.refresh' }) as HTMLButtonElement).disabled)
      .toBe(false);
    expect((screen.getByRole('button', { name: 'recovery.cleanup' }) as HTMLButtonElement).disabled)
      .toBe(true);
  });
});
