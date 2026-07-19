/* @vitest-environment jsdom */

import '@/test-utils';
import { act, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AgentConfigurationRequestRouter } from '../AgentConfigurationRequestRouter';

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  requestedCallback: null as null | ((event: { payload: { agentId: string } }) => void),
  guard: null as null | ReturnType<typeof vi.fn>,
}));

vi.mock('@/lifecycle/unsaved-changes-context', () => ({
  useOptionalUnsavedChanges: () => mocks.guard ? { guard: mocks.guard } : null,
}));

vi.mock('@/bindings', () => ({
  events: {
    agentConfigurationRequestedEvent: {
      listen: (callback: typeof mocks.requestedCallback) => {
        mocks.requestedCallback = callback;
        return mocks.listen(callback);
      },
    },
  },
}));

function Location() {
  const location = useLocation();
  return <div>{location.pathname}{location.search}</div>;
}

describe('AgentConfigurationRequestRouter', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.guard = null;
    mocks.requestedCallback = null;
    mocks.listen.mockResolvedValue(() => undefined);
  });

  it('navigates an incoming request to a prefilled Agent form', async () => {
    render(
      <MemoryRouter initialEntries={['/']}>
        <AgentConfigurationRequestRouter />
        <Location />
      </MemoryRouter>,
    );
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());

    act(() => mocks.requestedCallback?.({ payload: { agentId: 'new-agent' } }));

    await waitFor(() => expect(screen.getByText(
      '/settings?section=agents&view=new&configureAgent=new-agent',
    )).toBeDefined());
  });

  it('discards a request when the user keeps the current dirty draft', async () => {
    mocks.guard = vi.fn().mockResolvedValue(false);
    render(
      <MemoryRouter initialEntries={['/']}>
        <AgentConfigurationRequestRouter />
        <Location />
      </MemoryRouter>,
    );
    await waitFor(() => expect(mocks.listen).toHaveBeenCalled());

    act(() => mocks.requestedCallback?.({ payload: { agentId: 'new-agent' } }));

    await waitFor(() => expect(mocks.guard).toHaveBeenCalledTimes(1));
    expect(screen.getByText('/')).toBeDefined();
  });
});
