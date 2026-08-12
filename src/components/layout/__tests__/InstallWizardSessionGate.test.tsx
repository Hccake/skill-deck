/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { InstallWizardSessionGate } from '../InstallWizardSessionGate';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe('InstallWizardSessionGate', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: true,
      hasConfirmedSnapshot: false,
      syncError: null,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('delays startup feedback inside the fixed main region until 300ms', () => {
    render(
      <InstallWizardSessionGate>
        <main>main-content</main>
      </InstallWizardSessionGate>,
    );

    expect(screen.queryByText('main-content')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByRole('main')).toBeDefined();

    act(() => vi.advanceTimersByTime(299));
    expect(screen.queryByRole('status')).toBeNull();

    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByRole('status').textContent)
      .toBe('installWizardSession.startupDescription');
  });

  it('reveals content after the first snapshot and keeps it during background refresh', () => {
    render(
      <InstallWizardSessionGate>
        <main>main-content</main>
      </InstallWizardSessionGate>,
    );

    act(() => useInstallWizardSessionStore.setState({
      hasConfirmedSnapshot: true,
      loading: false,
    }));
    expect(screen.getByText('main-content')).toBeDefined();

    act(() => useInstallWizardSessionStore.setState({ loading: true }));
    expect(screen.getByText('main-content')).toBeDefined();
    expect(screen.queryByText('installWizardSession.startupDescription')).toBeNull();
  });
});
