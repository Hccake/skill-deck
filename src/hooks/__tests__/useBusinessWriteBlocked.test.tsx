/* @vitest-environment jsdom */

import '@/test-utils';
import { act, renderHook } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useMutationStore } from '@/stores/mutation';
import {
  businessWriteBlockReason,
  isBusinessWriteBlocked,
  useBusinessWriteBlocked,
} from '../useBusinessWriteBlocked';

describe('useBusinessWriteBlocked', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null });
    useInstallWizardSessionStore.setState({
      revision: 0,
      active: false,
      loading: false,
      syncError: null,
    });
  });

  it('blocks main-window writes for either a mutation or an active install wizard', () => {
    const { result } = renderHook(() => useBusinessWriteBlocked());
    expect(result.current).toBe(false);
    expect(isBusinessWriteBlocked()).toBe(false);

    act(() => useInstallWizardSessionStore.setState({ revision: 1, active: true }));
    expect(result.current).toBe(true);
    expect(isBusinessWriteBlocked()).toBe(true);
    expect(businessWriteBlockReason()).toBe('installWizardActive');

    act(() => {
      useInstallWizardSessionStore.setState({ revision: 2, active: false });
      useMutationStore.setState({ activeMutation: { id: 'busy' } as never });
    });
    expect(result.current).toBe(true);
    expect(businessWriteBlockReason()).toBe('mutationActive');
  });

  it('fails closed until the install wizard session is synchronized', () => {
    useInstallWizardSessionStore.setState({ loading: true });
    const { result } = renderHook(() => useBusinessWriteBlocked());

    expect(result.current).toBe(true);
    expect(isBusinessWriteBlocked()).toBe(true);
    expect(businessWriteBlockReason()).toBe('installWizardSyncing');

    act(() => {
      useInstallWizardSessionStore.setState({ loading: false, syncError: 'refresh' });
    });
    expect(result.current).toBe(true);
    expect(isBusinessWriteBlocked()).toBe(true);
    expect(businessWriteBlockReason()).toBe('installWizardUnavailable');

    act(() => {
      useInstallWizardSessionStore.setState({ syncError: null });
    });
    expect(result.current).toBe(false);
    expect(isBusinessWriteBlocked()).toBe(false);
  });
});
