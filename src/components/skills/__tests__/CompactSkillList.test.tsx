/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CompactSkillList } from '../CompactSkillList';
import type { InstalledSkill } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';

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
        context: { environment: { kind: 'native' }, scope: { scope: 'global' } },
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
});
