/* @vitest-environment jsdom */

import '@/test-utils';
import { describe, expect, it, beforeEach, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { GitCloneTimeoutSection } from '../GitCloneTimeoutSection';

const mockGetConfig = vi.fn();
const mockSaveConfig = vi.fn();

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, defaultValueOrOptions?: string | Record<string, unknown>) => {
      if (key === 'settings.cloneTimeout.currentValue') {
        const options = typeof defaultValueOrOptions === 'object' ? defaultValueOrOptions : undefined;
        return `Current: ${options?.seconds}s`;
      }

      if (typeof defaultValueOrOptions === 'string') {
        return defaultValueOrOptions;
      }

      return key;
    },
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getConfig: (...args: unknown[]) => mockGetConfig(...args),
  saveConfig: (...args: unknown[]) => mockSaveConfig(...args),
}));

describe('GitCloneTimeoutSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it('loads the persisted timeout and highlights the matching preset', async () => {
    mockGetConfig.mockResolvedValue({
      projects: [],
      gitCloneTimeoutSecs: 300,
    });

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('5 min');
    });
  });

  it('saves immediately when a preset is selected', async () => {
    mockGetConfig.mockResolvedValue({
      projects: ['/demo'],
      gitCloneTimeoutSecs: 120,
    });
    mockSaveConfig.mockResolvedValue(undefined);

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('2 min');
    });
    fireEvent.click(screen.getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: '10 min' }));

    await waitFor(() => {
      expect(mockSaveConfig).toHaveBeenCalledWith({
        projects: ['/demo'],
        gitCloneTimeoutSecs: 600,
      });
    });
  });

  it('shows and validates the custom seconds input', async () => {
    mockGetConfig.mockResolvedValue({
      projects: [],
      gitCloneTimeoutSecs: 120,
    });
    mockSaveConfig.mockResolvedValue(undefined);

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('2 min');
    });
    fireEvent.click(screen.getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: 'Custom' }));

    const input = screen.getByLabelText('Custom timeout');
    fireEvent.change(input, { target: { value: '20' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('Must be at least 30 seconds')).toBeTruthy();

    fireEvent.change(input, { target: { value: '4000' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(screen.getByText('Cannot exceed 3600 seconds')).toBeTruthy();

    fireEvent.change(input, { target: { value: '300' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(mockSaveConfig).toHaveBeenCalledWith({
        projects: [],
        gitCloneTimeoutSecs: 300,
      });
    });
  });

  it('restores the default timeout', async () => {
    mockGetConfig.mockResolvedValue({
      projects: [],
      gitCloneTimeoutSecs: 300,
    });
    mockSaveConfig.mockResolvedValue(undefined);

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('5 min');
    });
    fireEvent.click(screen.getByTitle('Restore default'));

    await waitFor(() => {
      expect(mockSaveConfig).toHaveBeenCalledWith({
        projects: [],
        gitCloneTimeoutSecs: 120,
      });
    });
  });

  it('does not render custom controls unless custom mode is active', async () => {
    mockGetConfig.mockResolvedValue({
      projects: [],
      gitCloneTimeoutSecs: 120,
    });

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('2 min');
    });
    expect(screen.queryByTestId('clone-timeout-advanced')).toBeNull();
    expect(screen.queryByLabelText('Custom timeout')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
  });

  it('keeps the persisted preset selected when preset save fails', async () => {
    mockGetConfig.mockResolvedValue({
      projects: ['/demo'],
      gitCloneTimeoutSecs: 120,
    });
    mockSaveConfig.mockRejectedValue(new Error('save failed'));

    render(<GitCloneTimeoutSection />);

    await waitFor(() => {
      expect(screen.getByRole('combobox').textContent).toContain('2 min');
    });
    fireEvent.click(screen.getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: '10 min' }));

    await waitFor(() => {
      expect(mockSaveConfig).toHaveBeenCalledWith({
        projects: ['/demo'],
        gitCloneTimeoutSecs: 600,
      });
    });

    expect(screen.getByRole('combobox').textContent).toContain('2 min');
    expect(screen.getByText('Failed to save timeout setting')).toBeTruthy();
  });
});
