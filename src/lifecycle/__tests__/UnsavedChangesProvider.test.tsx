/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { createMemoryRouter, RouterProvider, useLocation } from 'react-router-dom';
import {
  UnsavedChangesProvider,
} from '../UnsavedChangesProvider';
import {
  useRegisterUnsavedChanges,
  useUnsavedChanges,
} from '../unsaved-changes-context';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function Harness({
  discard,
  action,
}: {
  discard: () => void | Promise<void>;
  action: () => void | Promise<void>;
}) {
  useRegisterUnsavedChanges({ dirty: true, discard });
  const { guard } = useUnsavedChanges();
  return (
    <button type="button" onClick={() => void guard(action)}>
      leave
    </button>
  );
}

function LocationProbe({ discard }: { discard: () => void }) {
  useRegisterUnsavedChanges({ dirty: true, discard });
  const location = useLocation();
  return <div data-testid="location">{location.pathname}</div>;
}

describe('UnsavedChangesProvider', () => {
  it('keeps the draft on Stay and runs one queued action only after Discard', async () => {
    const discard = vi.fn();
    const action = vi.fn();
    const router = createMemoryRouter([{
      path: '*',
      element: (
        <UnsavedChangesProvider>
          <Harness discard={discard} action={action} />
        </UnsavedChangesProvider>
      ),
    }]);
    render(<RouterProvider router={router} />);

    fireEvent.click(screen.getByRole('button', { name: 'leave' }));
    fireEvent.click(await screen.findByRole('button', { name: 'settings.agents.dirtyNavigation.stay' }));
    expect(discard).not.toHaveBeenCalled();
    expect(action).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'leave' }));
    fireEvent.click(await screen.findByRole('button', { name: 'settings.agents.dirtyNavigation.discard' }));

    await waitFor(() => expect(discard).toHaveBeenCalledTimes(1));
    expect(action).toHaveBeenCalledTimes(1);
  });

  it('guards browser history navigation with the same Stay and Discard decision', async () => {
    const discard = vi.fn();
    const router = createMemoryRouter([{
      path: '*',
      element: (
        <UnsavedChangesProvider>
          <LocationProbe discard={discard} />
        </UnsavedChangesProvider>
      ),
    }], { initialEntries: ['/first', '/second'], initialIndex: 1 });
    render(<RouterProvider router={router} />);

    await act(async () => { await router.navigate(-1); });
    expect(screen.getByTestId('location').textContent).toBe('/second');
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.stay',
    }));
    expect(screen.getByTestId('location').textContent).toBe('/second');
    expect(discard).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole('button', {
      name: 'settings.agents.dirtyNavigation.stay',
    })).toBeNull());

    await act(async () => { await router.navigate(-1); });
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));

    await waitFor(() => expect(screen.getByTestId('location').textContent).toBe('/first'));
    expect(discard).toHaveBeenCalledTimes(1);
  });

  it('resolves a failed discard as not performed and continues the queued guard', async () => {
    const discard = vi.fn()
      .mockRejectedValueOnce(new Error('discard failed'))
      .mockResolvedValue(undefined);
    const action = vi.fn();
    const router = createMemoryRouter([{
      path: '*',
      element: (
        <UnsavedChangesProvider>
          <Harness discard={discard} action={action} />
        </UnsavedChangesProvider>
      ),
    }]);
    render(<RouterProvider router={router} />);

    fireEvent.click(screen.getByRole('button', { name: 'leave' }));
    fireEvent.click(screen.getByText('leave'));
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));

    await waitFor(() => expect(discard).toHaveBeenCalledTimes(1));
    expect(action).not.toHaveBeenCalled();

    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));
    await waitFor(() => expect(discard).toHaveBeenCalledTimes(2));
    expect(action).toHaveBeenCalledTimes(1);
  });

  it('resolves a failed action as not performed and continues the queued guard', async () => {
    const discard = vi.fn();
    const action = vi.fn()
      .mockRejectedValueOnce(new Error('action failed'))
      .mockResolvedValue(undefined);
    const router = createMemoryRouter([{
      path: '*',
      element: (
        <UnsavedChangesProvider>
          <Harness discard={discard} action={action} />
        </UnsavedChangesProvider>
      ),
    }]);
    render(<RouterProvider router={router} />);

    fireEvent.click(screen.getByRole('button', { name: 'leave' }));
    fireEvent.click(screen.getByText('leave'));
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));

    await waitFor(() => expect(action).toHaveBeenCalledTimes(1));
    fireEvent.click(await screen.findByRole('button', {
      name: 'settings.agents.dirtyNavigation.discard',
    }));
    await waitFor(() => expect(action).toHaveBeenCalledTimes(2));
  });
});
