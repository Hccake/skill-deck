/* @vitest-environment jsdom */

import '@/test-utils';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';
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
vi.mock('../WindowControls', () => ({ WindowControls: () => <div data-testid="window-controls" /> }));
vi.mock('@/components/recovery/RecoveryCenter', () => ({ RecoveryCenter: () => null }));
vi.mock('@/assets/logo.png', () => ({ default: 'logo.png' }));

describe('Header', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('exposes the four primary destinations and application title', () => {
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

    expect(screen.getByText('app.name')).toBeTruthy();
  });

  it.each([
    ['Windows', 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)', true],
    ['macOS', 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', false],
    ['Linux', 'Mozilla/5.0 (X11; Linux x86_64)', false],
  ])('uses the expected window controls on %s', (_platform, userAgent, customControls) => {
    vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(userAgent);

    render(<MemoryRouter><Header /></MemoryRouter>);

    expect(screen.queryByTestId('window-controls') !== null).toBe(customControls);
  });

  it('keeps navigation gaps draggable and excludes interactive controls', () => {
    const { container } = render(<MemoryRouter><Header /></MemoryRouter>);

    expect(screen.getByRole('banner').hasAttribute('data-tauri-drag-region')).toBe(true);
    expect(['', 'true']).toContain(screen.getByRole('navigation').getAttribute('data-tauri-drag-region'));
    for (const link of screen.getAllByRole('link')) {
      expect(link.getAttribute('data-tauri-drag-region')).toBe('false');
    }
    const tools = screen.getByRole('button', { name: 'theme.dark' }).parentElement!;
    expect(['', 'true']).toContain(tools.getAttribute('data-tauri-drag-region'));
    for (const button of tools.querySelectorAll('button')) {
      expect(button.hasAttribute('data-tauri-drag-region')).toBe(false);
    }
    for (const region of container.querySelectorAll('[data-tauri-drag-region]')) {
      expect(['', 'true', 'false']).toContain(region.getAttribute('data-tauri-drag-region'));
    }
  });
});
