/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { CustomAgentDefinition } from '@/bindings';
import { AgentDefinitionForm } from '../AgentDefinitionForm';

Element.prototype.scrollIntoView = vi.fn();

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const draft: CustomAgentDefinition = {
  id: 'my-agent',
  displayName: 'My Agent',
  global: {
    enabled: true,
    location: 'both',
    privatePath: { kind: 'based', base: 'configHome', relativePath: 'my-agent/skills' },
  },
  project: {
    enabled: true,
    location: 'private',
    privatePath: { kind: 'based', base: 'project', relativePath: '.my-agent/skills' },
  },
  detectionPaths: [{ kind: 'based', base: 'home', relativePath: '.my-agent' }],
};

function renderForm(overrides: Partial<React.ComponentProps<typeof AgentDefinitionForm>> = {}) {
  const props: React.ComponentProps<typeof AgentDefinitionForm> = {
    draft,
    originalId: 'my-agent',
    errors: [],
    disabled: false,
    stale: false,
    onChange: vi.fn(),
    onReload: vi.fn(),
    ...overrides,
  };
  return { ...render(<AgentDefinitionForm {...props} />), props };
}

describe('AgentDefinitionForm', () => {
  it('uses scope switches and select-based read modes', () => {
    renderForm();

    expect(screen.getByRole('switch', { name: 'settings.agents.global.enabled' })).toBeDefined();
    expect(screen.getByRole('switch', { name: 'settings.agents.project.enabled' })).toBeDefined();
    expect(screen.queryAllByRole('radio')).toHaveLength(0);
    expect(screen.getByRole('combobox', { name: 'settings.agents.global.location' }).textContent)
      .toContain('settings.agents.locations.both');
    expect(screen.getByRole('combobox', { name: 'settings.agents.project.location' }).textContent)
      .toContain('settings.agents.locations.private');
    expect(screen.getByText('settings.agents.skillReading.title')).toBeDefined();
    expect(screen.getByText('settings.agents.installDetection.title')).toBeDefined();
  });

  it('collapses a disabled scope while preserving its existing private-path draft', () => {
    const onChange = vi.fn();
    const { rerender, props } = renderForm({ onChange });

    fireEvent.click(screen.getByRole('switch', { name: 'settings.agents.global.enabled' }));
    expect(onChange).toHaveBeenCalledWith({
      ...draft,
      global: { ...draft.global, enabled: false },
    });

    rerender(<AgentDefinitionForm {...props} draft={{
      ...draft,
      global: { ...draft.global, enabled: false },
    }} />);
    expect(screen.getByText('settings.agents.readMode.globalUnsupported')).toBeDefined();
    expect(screen.queryByDisplayValue('my-agent/skills')).toBeNull();

    fireEvent.click(screen.getByRole('switch', { name: 'settings.agents.global.enabled' }));
    expect(onChange).toHaveBeenLastCalledWith(draft);
  });

  it('keeps Project editing relative to Project without rendering a resolved preview', () => {
    renderForm();

    expect(screen.queryByLabelText('settings.agents.project.pathKind')).toBeNull();
    expect(screen.queryByLabelText('settings.agents.project.pathBase')).toBeNull();
    expect(screen.getByLabelText('settings.agents.project.relativePath')).toBeDefined();
    expect(screen.queryByText('<Project>/.my-agent/skills')).toBeNull();
  });

  it('uses one directory-location control instead of exposing based-path terminology', () => {
    renderForm();

    expect(screen.getByLabelText('settings.agents.global.directoryLocation')).toBeDefined();
    expect(screen.queryByLabelText('settings.agents.global.pathKind')).toBeNull();
    expect(screen.queryByText('settings.agents.pathKinds.based')).toBeNull();
  });

  it('restores the ID-derived private path when a shared scope becomes private', () => {
    const onChange = vi.fn();
    renderForm({
      draft: {
        ...draft,
        global: { enabled: true, location: 'shared', privatePath: null },
      },
      onChange,
    });

    fireEvent.click(screen.getByRole('combobox', { name: 'settings.agents.global.location' }));
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.locations.private' }));

    expect(onChange).toHaveBeenCalledWith({
      ...draft,
      global: {
        enabled: true,
        location: 'private',
        privatePath: { kind: 'based', base: 'home', relativePath: '.my-agent/skills' },
      },
    });
  });
});
