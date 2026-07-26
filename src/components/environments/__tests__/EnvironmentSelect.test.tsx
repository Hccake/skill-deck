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
function renderSelect(overrides: Partial<React.ComponentProps<typeof EnvironmentSelect>> = {}) {
  const props: React.ComponentProps<typeof EnvironmentSelect> = {
    environments: [host],
    value: host.environment,
    onChange: vi.fn(),
    pendingEnvironment: null,
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

  it('keeps pending feedback inside the existing trigger', () => {
    renderSelect({
      environments: [host, ubuntu],
      pendingEnvironment: ubuntu.environment,
    });

    const select = screen.getByRole('combobox', { name: 'context.environmentLabel' });
    expect(select.getAttribute('data-slot')).toBe('select-trigger');
    expect((select as HTMLSelectElement).disabled).toBe(true);
    expect(select.getAttribute('aria-busy')).toBe('true');
    const status = screen.getByRole('status');
    expect(status.closest('[data-slot="select-trigger"]')).toBe(select);
    expect(status.textContent).toContain(
      'context.environmentConnectingTo:Ubuntu 24.04 Long Environment Name',
    );
  });

  it('keeps a non-current failed distribution in the selector without a persistent alert', () => {
    const connectionError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution is stopped' },
    };
    renderSelect({
      environments: [host, { ...ubuntu, status: 'unavailable', error: connectionError }],
    });

    expect(screen.queryByRole('status')).toBeNull();
    fireEvent.click(screen.getByRole('combobox', { name: 'context.environmentLabel' }));
    expect(screen.getByRole('option', {
      name: /Ubuntu 24\.04 Long Environment Name/,
    }).getAttribute('title')).toBe('Ubuntu 24.04 Long Environment Name');
  });

  it('shows a typed discovery error without adding an independent retry', () => {
    const discoveryError: AppError = {
      kind: 'environmentDiscoveryFailed',
      data: { message: 'wsl.exe unavailable' },
    };
    renderSelect({
      environments: [host],
      discoveryError,
    });

    expect(screen.getByText('context.environmentDiscoveryFailed')).toBeDefined();
    expect(screen.getByText('addSkill.error.environmentDiscoveryFailed')).toBeDefined();
    expect(screen.queryByRole('button', { name: 'context.environmentRetry' })).toBeNull();
  });

  it('shows a reconnect action only for the selected failed environment', () => {
    const onChange = vi.fn();
    const connectionError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution is stopped' },
    };
    renderSelect({
      environments: [host, { ...ubuntu, status: 'unavailable', error: connectionError }],
      value: ubuntu.environment,
      onChange,
    });

    expect(screen.getByText('context.environmentConnectionFailed:Ubuntu 24.04 Long Environment Name'))
      .toBeDefined();
    expect(screen.getByText('addSkill.error.environmentUnavailable')).toBeDefined();
    fireEvent.click(screen.getByRole('button', {
      name: 'context.environmentRetryNamed:Ubuntu 24.04 Long Environment Name',
    }));
    expect(onChange).toHaveBeenCalledWith(ubuntu.environment);
  });

  it('retries a non-current failed environment through normal selection', () => {
    const onChange = vi.fn();
    const connectionError: AppError = {
      kind: 'environmentUnavailable',
      data: { environment: ubuntu.environment, message: 'distribution is stopped' },
    };
    renderSelect({
      environments: [host, { ...ubuntu, status: 'unavailable', error: connectionError }],
      onChange,
    });

    fireEvent.click(screen.getByRole('combobox', { name: 'context.environmentLabel' }));
    fireEvent.click(screen.getByRole('option', {
      name: /Ubuntu 24\.04 Long Environment Name/,
    }));

    expect(onChange).toHaveBeenCalledWith(ubuntu.environment);
  });
});
