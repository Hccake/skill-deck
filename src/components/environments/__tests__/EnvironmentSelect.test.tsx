/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { AppError, EnvironmentInfo } from '@/bindings';
import { EnvironmentSelect } from '../EnvironmentSelect';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { environment?: string }) => (
      options?.environment ? `${key}:${options.environment}` : key
    ),
  }),
}));

const host: EnvironmentInfo = {
  environment: { kind: 'host' },
  displayName: 'Windows',
  status: 'available',
  revision: 1,
  error: null,
};
const ubuntu: EnvironmentInfo = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu-24.04-Long-Environment-Name' },
  displayName: 'Ubuntu 24.04 Long Environment Name',
  status: 'available',
  revision: 1,
  error: null,
};
const discoveryError: AppError = {
  kind: 'environmentDiscoveryFailed',
  data: { message: 'wsl.exe timed out' },
};
const connectionError: AppError = {
  kind: 'environmentUnavailable',
  data: {
    environment: ubuntu.environment,
    message: 'distribution is unavailable',
  },
};

function renderSelect(overrides: Partial<React.ComponentProps<typeof EnvironmentSelect>> = {}) {
  const props: React.ComponentProps<typeof EnvironmentSelect> = {
    environments: [host],
    value: host.environment,
    onChange: vi.fn(),
    discoveryState: 'ready',
    discoveryError: null,
    connectionErrors: {},
    pendingEnvironment: null,
    onRetryDiscovery: vi.fn(),
    onRetryConnection: vi.fn(),
    ...overrides,
  };
  return { ...render(<EnvironmentSelect {...props} />), props };
}

describe('EnvironmentSelect', () => {
  it('renders no environment UI for a normal Host-only snapshot', () => {
    renderSelect();

    expect(screen.queryByRole('combobox')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('keeps discovery failure recoverable when only Host is available', () => {
    const onRetryDiscovery = vi.fn();
    renderSelect({
      discoveryState: 'error',
      discoveryError,
      onRetryDiscovery,
    });

    expect(screen.getByRole('status').textContent).toContain('context.environmentDiscoveryFailed');
    fireEvent.click(screen.getByRole('button', { name: 'context.environmentRetry' }));
    expect(onRetryDiscovery).toHaveBeenCalledTimes(1);
  });

  it('announces and names the pending environment switch', () => {
    renderSelect({
      environments: [host, ubuntu],
      pendingEnvironment: ubuntu.environment,
    });

    const select = screen.getByRole('combobox', { name: 'context.environmentLabel' });
    expect(select.getAttribute('data-slot')).toBe('select-trigger');
    expect((select as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByRole('status').getAttribute('aria-live')).toBe('polite');
    expect(screen.getByRole('status').textContent).toContain(
      'context.environmentConnectingTo:Ubuntu 24.04 Long Environment Name',
    );
  });

  it('offers retry for the failed distribution and preserves full option text', () => {
    const onRetryConnection = vi.fn();
    renderSelect({
      environments: [host, { ...ubuntu, status: 'unavailable' }],
      connectionErrors: {
        'wsl:ubuntu-24.04-long-environment-name': connectionError,
      },
      onRetryConnection,
    });

    expect(screen.getByRole('status').textContent).toContain(
      'context.environmentConnectionFailed:Ubuntu 24.04 Long Environment Name',
    );
    fireEvent.click(screen.getByRole('button', {
      name: 'context.environmentRetryNamed:Ubuntu 24.04 Long Environment Name',
    }));
    expect(onRetryConnection).toHaveBeenCalledWith(ubuntu.environment);
    fireEvent.click(screen.getByRole('combobox', { name: 'context.environmentLabel' }));
    expect(screen.getByRole('option', {
      name: /Ubuntu 24\.04 Long Environment Name/,
    }).getAttribute('title')).toBe('Ubuntu 24.04 Long Environment Name');
  });

  it('selects an environment through the shadcn Select contract', () => {
    const onChange = vi.fn();
    renderSelect({ environments: [host, ubuntu], onChange });

    fireEvent.click(screen.getByRole('combobox', { name: 'context.environmentLabel' }));
    fireEvent.click(screen.getByRole('option', {
      name: 'Ubuntu 24.04 Long Environment Name',
    }));

    expect(onChange).toHaveBeenCalledWith(ubuntu.environment);
  });
});
