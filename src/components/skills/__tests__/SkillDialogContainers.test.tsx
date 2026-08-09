/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ManageAgentsDialogContainer } from '../ManageAgentsDialogContainer';
import { CopyToProjectDialogContainer } from '../CopyToProjectDialogContainer';
import { UpdatePlanDialogContainer } from '../UpdatePlanDialogContainer';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import type { SkillLocationRef, InstalledSkill } from '@/bindings';

const mocks = vi.hoisted(() => ({
  getCopyAgentSelection: vi.fn(),
  getManageAgentSelection: vi.fn(),
}));

vi.mock('@/workflows/skill-manage-agents', () => ({
  executeManageAgentChanges: vi.fn(),
}));

vi.mock('@/workflows/skill-copy', () => ({
  executeSkillCopy: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  getCopyAgentSelection: (...args: unknown[]) => mocks.getCopyAgentSelection(...args),
  getManageAgentSelection: (...args: unknown[]) => mocks.getManageAgentSelection(...args),
  listSkills: vi.fn(),
}));

vi.mock('../ManageAgentsDialog', () => ({
  ManageAgentsDialog: (props: {
    context: SkillLocationRef;
    skill: InstalledSkill;
    loadAgentSelection: (request: {
      kind: 'manage';
      context: SkillLocationRef;
      skillName: string;
    }) => Promise<unknown>;
  }) => (
    <div data-testid="manage-container-dialog">
      <button
        type="button"
        onClick={() => void props.loadAgentSelection({
          kind: 'manage',
          context: props.context,
          skillName: props.skill.name,
        })}
      >
        load
      </button>
    </div>
  ),
}));

vi.mock('../CopyToProjectDialog', () => ({
  CopyToProjectDialog: (props: {
    open: boolean;
    skill: InstalledSkill;
    sourceContext: SkillLocationRef;
  }) => (
    <div
      data-testid="copy-container-dialog"
      data-open={String(props.open)}
      data-skill={props.skill.name}
      data-project={props.sourceContext.scope.scope === 'project'
        ? props.sourceContext.scope.project_id
        : 'global'}
    />
  ),
}));

vi.mock('../UpdatePlanDialog', () => ({
  UpdatePlanDialog: (props: { open: boolean; skillNames: string[] }) => (
    <div
      data-testid="update-container-dialog"
      data-open={String(props.open)}
      data-skills={props.skillNames.join(',')}
    />
  ),
}));

const context: SkillLocationRef = {
  environment: { kind: 'native' },
  scope: { scope: 'project', project_id: 'source-project' },
};

const skill: InstalledSkill = {
  name: 'toolkit',
  description: '',
  path: '/project/.agents/skills/toolkit',
  canonicalPath: '/project/.agents/skills/toolkit',
  scope: 'project',
  agents: [],
  associatedAgents: [],
  source: 'owner/repo',
};

describe('Skill dialog containers', () => {
  beforeEach(() => {
    useSkillDialogStore.getState().closeManageAgents();
    useSkillDialogStore.getState().closeCopyToProject();
    useSkillDialogStore.getState().closeRepairSource();
    useSkillUpdateWorkflow.getState().reset();
  });

  it('keeps Manage Agents loading bound to the opened dialog session', () => {
    useSkillDialogStore.getState().openManageAgents(skill, context);

    render(<ManageAgentsDialogContainer />);

    fireEvent.click(screen.getByRole('button', { name: 'load' }));
    expect(mocks.getManageAgentSelection).toHaveBeenCalledWith(context, skill.name);
  });

  it('mounts Copy to Project only for a complete dialog session', () => {
    const { rerender } = render(<CopyToProjectDialogContainer />);
    expect(screen.queryByTestId('copy-container-dialog')).toBeNull();

    act(() => {
      useSkillDialogStore.getState().openCopyToProject(skill, context);
    });
    rerender(<CopyToProjectDialogContainer />);

    const dialog = screen.getByTestId('copy-container-dialog');
    expect(dialog.dataset.skill).toBe('toolkit');
    expect(dialog.dataset.project).toBe('source-project');
  });

  it('mounts Update Plan only while the workflow owns an open session', () => {
    render(<UpdatePlanDialogContainer />);
    expect(screen.queryByTestId('update-container-dialog')).toBeNull();

    act(() => {
      useSkillUpdateWorkflow.setState({
        phase: 'loadingPreview',
        context,
        skillNames: ['toolkit'],
      });
    });

    const dialog = screen.getByTestId('update-container-dialog');
    expect(dialog.dataset.open).toBe('true');
    expect(dialog.dataset.skills).toBe('toolkit');
  });
});
