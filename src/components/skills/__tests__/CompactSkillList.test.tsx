/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render as testingRender, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { CompactSkillList } from '../CompactSkillList';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

const render = (ui: React.ReactElement) => testingRender(<MemoryRouter>{ui}</MemoryRouter>);

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

function makeSkill(name: string): InstalledSkill {
  return {
    name,
    description: `${name} description`,
    path: `/tmp/${name}`,
    canonicalPath: `/tmp/.agents/${name}`,
    scope: 'global',
    agents: ['codex'],
    associatedAgents: ['codex'],
    source: 'owner/repo',
    sourceUrl: 'https://github.com/owner/repo',
    installedAt: null,
    updatedAt: null,
    hasUpdate: false,
    pluginName: null,
    gitRef: null,
  };
}

describe('CompactSkillList', () => {
  beforeEach(() => {
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
  });

  it('disables compact add actions while another mutation is active', () => {
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        target: { kind: 'skillLocation', environment: { kind: 'native' }, scope: { scope: 'global' } },
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(
      <CompactSkillList
        globalSkills={[makeSkill('alpha')]}
        projectSkills={[]}
        selectedSkillRef={null}
        isProjectSelected={false}
        projectTitle="Project Skills"
        onAddGlobal={vi.fn()}
        onSkillClick={vi.fn()}
      />
    );

    expect((screen.getByTitle('skills.add') as HTMLButtonElement).disabled).toBe(true);
  });

  it('keeps Global and Project sections with their own add actions when filters match nothing', () => {
    render(
      <CompactSkillList
        globalSkills={[]}
        projectSkills={[]}
        selectedSkillRef={{ name: 'hidden', scope: 'project', projectPath: '/work/app' }}
        isProjectSelected
        projectTitle="Project Skills"
        projectPath="/work/app"
        onAddProject={vi.fn()}
        onAddGlobal={vi.fn()}
        onSkillClick={vi.fn()}
        projectEmptyState={<div>project-filter-empty</div>}
        globalEmptyState={<div>global-filter-empty</div>}
      />
    );

    expect(screen.getByText('Project Skills')).toBeDefined();
    expect(screen.getByText('skills.globalSkills')).toBeDefined();
    expect(screen.getByText('project-filter-empty')).toBeDefined();
    expect(screen.getByText('global-filter-empty')).toBeDefined();
    expect(screen.getAllByRole('button', { name: 'skills.add' })).toHaveLength(2);
  });

  it('shows every applied Library without an overflow control', () => {
    render(
      <CompactSkillList
        globalSkills={[]}
        projectSkills={[]}
        selectedSkillRef={null}
        isProjectSelected
        projectTitle="Project Skills"
        onSkillClick={vi.fn()}
        projectLibraryApplication={{
          orderedLibraries: [
            { id: 'project-a', name: 'Project A', skillCount: 1 },
            { id: 'project-b', name: 'Project B', skillCount: 1 },
            { id: 'project-c', name: 'Project C', skillCount: 1 },
          ],
          selectedAgentIds: [],
          pending: false,
        }}
        globalLibraryApplication={{
          orderedLibraries: [{ id: 'global-a', name: 'Global A', skillCount: 1 }],
          selectedAgentIds: [],
          pending: true,
        }}
        onManageProjectLibraries={vi.fn()}
        onManageGlobalLibraries={vi.fn()}
      />
    );

    const summaries = screen.getAllByTestId('applied-libraries-summary');
    expect(summaries).toHaveLength(2);
    const manageLibraries = screen.getAllByRole('button', { name: 'libraries.manage' });
    expect(manageLibraries).toHaveLength(2);
    expect(screen.queryByText('libraries.applied')).toBeNull();
    expect(within(summaries[0]).queryByRole('link')).toBeNull();
    const projectLibrary = within(summaries[0]).getAllByTestId('library-summary-item')[0];
    expect(within(projectLibrary).getByText('libraries.skillCount')).toBeTruthy();
    expect(screen.getByText('Project B')).toBeDefined();
    expect(screen.getByText('Project C')).toBeDefined();
    expect(screen.queryByRole('button', { name: /^\+\d+$/ })).toBeNull();
    expect(screen.getByRole('status').textContent).toBe('libraries.pending');
  });
});
