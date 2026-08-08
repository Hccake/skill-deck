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

  it('stretches its scroll area to the full available panel size', () => {
    const { container } = render(
      <div className="h-[480px]">
        <CompactSkillList
          globalSkills={[makeSkill('alpha'), makeSkill('beta')]}
          projectSkills={[]}
          selectedSkillRef={{ name: 'alpha', scope: 'global', projectPath: null }}
          isProjectSelected={false}
          projectTitle="Project Skills"
          projectPath="global"
          onSkillClick={() => undefined}
        />
      </div>
    );

    const scrollArea = container.querySelector('[data-slot="scroll-area"]');
    const viewport = container.querySelector('[data-slot="scroll-area-viewport"]');

    expect(scrollArea).not.toBeNull();
    expect(scrollArea?.className).toContain('absolute');
    expect(scrollArea?.className).toContain('inset-0');
    expect(scrollArea?.className).toContain('w-full');
    expect(scrollArea?.className).toContain('h-full');
    expect(viewport).not.toBeNull();
    expect(viewport?.className).toContain('[&>div]:!block');
    expect(viewport?.className).toContain('[&>div]:w-full');
    expect(viewport?.className).toContain('[&>div]:min-w-0');
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
