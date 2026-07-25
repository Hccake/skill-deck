/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { EnvironmentInfo } from '@/bindings';
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

  it('keeps an unavailable distribution in the selector without adding an error row', () => {
    renderSelect({
      environments: [host, { ...ubuntu, status: 'unavailable' }],
    });

    expect(screen.queryByRole('status')).toBeNull();
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
