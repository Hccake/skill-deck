/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { Header } from '../Header';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/stores/settings', () => ({
  useSettingsStore: (selector: (state: unknown) => unknown) => selector({
    theme: 'light',
    toggleTheme: vi.fn(),
    locale: 'en',
    setLocale: vi.fn(),
  }),
}));

vi.mock('@/lifecycle/unsaved-changes-context', () => ({
  useOptionalUnsavedChanges: () => null,
}));

vi.mock('../GlobalEnvironmentSwitcher', () => ({ GlobalEnvironmentSwitcher: () => null }));
vi.mock('../InstallWizardStatusControl', () => ({ InstallWizardStatusControl: () => null }));
vi.mock('@/components/recovery/RecoveryCenter', () => ({ RecoveryCenter: () => null }));
vi.mock('@/assets/logo.png', () => ({ default: 'logo.png' }));

describe('Header', () => {
  it('exposes the four primary destinations', () => {
    render(<MemoryRouter><Header /></MemoryRouter>);

    const destinations = [
      ['nav.skills', '/'],
      ['nav.libraries', '/libraries'],
      ['nav.discover', '/discover'],
      ['nav.settings', '/settings'],
    ] as const;
    for (const [name, href] of destinations) {
      const link = screen.getByRole('link', { name });
      expect(link.getAttribute('href')).toBe(href);
      expect(link.getAttribute('aria-label')).toBe(name);
    }
  });
});
