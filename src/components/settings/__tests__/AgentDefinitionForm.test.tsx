/* @vitest-environment jsdom */

import '@/test-utils';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
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

function StatefulForm({ initialDraft = draft }: { initialDraft?: CustomAgentDefinition }) {
  const [value, setValue] = useState(initialDraft);
  return (
    <AgentDefinitionForm
      draft={value}
      originalId="my-agent"
      errors={[]}
      disabled={false}
      stale={false}
      onChange={setValue}
      onReload={vi.fn()}
    />
  );
}

describe('AgentDefinitionForm', () => {
  it('uses scope switches and radio-based read rules under named scope sections', () => {
    renderForm();

    expect(screen.getByRole('switch', { name: 'settings.agents.global.enabled' })).toBeDefined();
    expect(screen.getByRole('switch', { name: 'settings.agents.project.enabled' })).toBeDefined();
    const globalSection = screen.getByRole('region', { name: 'settings.agents.global.readTitle' });
    const projectSection = screen.getByRole('region', { name: 'settings.agents.project.readTitle' });
    expect(within(globalSection).getAllByRole('radio')).toHaveLength(3);
    expect(within(projectSection).getAllByRole('radio')).toHaveLength(3);
    expect(within(globalSection).getByRole('radio', {
      name: 'settings.agents.locations.both',
    }).getAttribute('data-state')).toBe('checked');
    expect(within(projectSection).getByRole('radio', {
      name: 'settings.agents.locations.private',
    }).getAttribute('data-state')).toBe('checked');
    expect(screen.queryByRole('combobox', { name: 'settings.agents.global.location' })).toBeNull();
    expect(screen.queryByRole('combobox', { name: 'settings.agents.project.location' })).toBeNull();
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
    const projectSection = screen.getByRole('region', { name: 'settings.agents.project.readTitle' });
    expect(within(projectSection).getByLabelText('settings.agents.directoryKind.private')).toBeDefined();
    expect(screen.queryByText('<Project>/.my-agent/skills')).toBeNull();
  });

  it('uses one directory-location control instead of exposing based-path terminology', () => {
    renderForm();

    expect(screen.getByLabelText('settings.agents.global.directoryLocation')).toBeDefined();
    expect(screen.queryByLabelText('settings.agents.global.pathKind')).toBeNull();
    expect(screen.queryByText('settings.agents.pathKinds.based')).toBeNull();
  });

  it('marks identity fields as required and explains whether the Agent ID can change', () => {
    const { rerender, props } = renderForm({ originalId: null });

    expect(screen.getByRole('textbox', {
      name: /settings\.agents\.fields\.displayName/,
    }).hasAttribute('required')).toBe(true);
    const editableId = screen.getByRole('textbox', {
      name: /settings\.agents\.fields\.id/,
    });
    expect(editableId.hasAttribute('required')).toBe(true);
    expect(editableId.hasAttribute('readonly')).toBe(false);
    expect(screen.getByText('settings.agents.fields.idHint.generated')).toBeDefined();

    rerender(<AgentDefinitionForm {...props} originalId="my-agent" />);
    expect(screen.getByRole('textbox', {
      name: /settings\.agents\.fields\.id/,
    }).hasAttribute('readonly')).toBe(true);
    expect(screen.getByText('settings.agents.fields.idHint.locked')).toBeDefined();
  });

  it('uses equal-height label rows for the Agent name and ID fields', () => {
    renderForm();

    const nameLabel = screen.getByText('settings.agents.fields.displayName');
    const idLabel = screen.getByText('settings.agents.fields.id');
    expect(nameLabel.parentElement?.className).toContain('min-h-5');
    expect(idLabel.parentElement?.className).toContain('min-h-5');
  });

  it('shows one group-level error for the Skill read scopes', () => {
    renderForm({ errors: [{ field: 'scopes', code: 'required' }] });

    expect(screen.getAllByText('settings.agents.validation.required')).toHaveLength(1);
  });

  it('combines a directory location, visible prefix and path while preserving each path kind draft', () => {
    render(<StatefulForm />);

    const globalSection = screen.getByRole('region', { name: 'settings.agents.global.readTitle' });
    const location = within(globalSection).getByRole('combobox', {
      name: 'settings.agents.global.directoryLocation',
    });
    const path = within(globalSection).getByRole('textbox', {
      name: 'settings.agents.directoryKind.private',
    }) as HTMLInputElement;
    expect(within(globalSection).getByText('~/.config/')).toBeDefined();
    expect(path.value).toBe('my-agent/skills');

    fireEvent.click(location);
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));
    fireEvent.change(path, { target: { value: '/opt/nova/skills' } });

    fireEvent.click(location);
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.home' }));
    expect(path.value).toBe('my-agent/skills');
    expect(within(globalSection).getByText('~/')).toBeDefined();

    fireEvent.click(location);
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));
    expect(path.value).toBe('/opt/nova/skills');
  });

  it('names detection paths, focuses a newly added path and announces the change', async () => {
    render(<StatefulForm />);

    expect(screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    })).toBeDefined();
    fireEvent.click(screen.getByRole('button', { name: 'settings.agents.detection.add' }));

    const secondGroup = screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 2',
    });
    const secondPath = within(secondGroup).getByRole('textbox', {
      name: 'settings.agents.detection.pathInput 2',
    });
    await waitFor(() => expect(document.activeElement).toBe(secondPath));
    expect(screen.getByRole('status').textContent)
      .toContain('settings.agents.detection.added 2');
  });

  it('does not reuse a removed detection row\'s alternate path draft', () => {
    render(<StatefulForm initialDraft={{
      ...draft,
      detectionPaths: [
        { kind: 'based', base: 'home', relativePath: '.first-agent' },
        { kind: 'based', base: 'home', relativePath: '.second-agent' },
      ],
    }} />);

    const firstGroup = screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    });
    const firstLocation = within(firstGroup).getByRole('combobox');
    const firstPath = within(firstGroup).getByRole('textbox') as HTMLInputElement;
    fireEvent.click(firstLocation);
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));
    fireEvent.change(firstPath, { target: { value: '/removed-agent' } });
    fireEvent.click(firstLocation);
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.home' }));
    fireEvent.click(within(firstGroup).getByRole('button', {
      name: 'settings.agents.detection.remove 1',
    }));

    const remainingGroup = screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    });
    fireEvent.click(within(remainingGroup).getByRole('combobox'));
    fireEvent.click(screen.getByRole('option', { name: 'settings.agents.pathLocations.absolute' }));
    expect((within(remainingGroup).getByRole('textbox') as HTMLInputElement).value).toBe('');
  });

  it('announces repeated removal of the same path position as separate updates', () => {
    render(<StatefulForm initialDraft={{
      ...draft,
      detectionPaths: [
        { kind: 'based', base: 'home', relativePath: '.first-agent' },
        { kind: 'based', base: 'home', relativePath: '.second-agent' },
        { kind: 'based', base: 'home', relativePath: '.third-agent' },
      ],
    }} />);

    fireEvent.click(within(screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    })).getByRole('button', { name: 'settings.agents.detection.remove 1' }));
    const status = screen.getByRole('status');
    const firstAnnouncement = status.firstChild;

    fireEvent.click(within(screen.getByRole('group', {
      name: 'settings.agents.detection.pathLabel 1',
    })).getByRole('button', { name: 'settings.agents.detection.remove 1' }));

    expect(status.textContent).toBe('settings.agents.detection.removed 1');
    expect(status.firstChild).not.toBe(firstAnnouncement);
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

    const globalSection = screen.getByRole('region', { name: 'settings.agents.global.readTitle' });
    fireEvent.click(within(globalSection).getByRole('radio', {
      name: 'settings.agents.locations.private',
    }));

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
