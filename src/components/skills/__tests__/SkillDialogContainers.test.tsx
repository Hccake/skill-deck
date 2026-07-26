/* @vitest-environment jsdom */

import '@/test-utils';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ManageAgentsDialogContainer } from '../ManageAgentsDialogContainer';
import { CopyToProjectDialogContainer } from '../CopyToProjectDialogContainer';
import { UpdatePlanDialogContainer } from '../UpdatePlanDialogContainer';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import type { ContextRef, InstalledSkill } from '@/bindings';

const mocks = vi.hoisted(() => ({
  openManageAgentChanges: vi.fn(),
}));

vi.mock('@/workflows/skill-manage-agents', () => ({
  openManageAgentChanges: mocks.openManageAgentChanges,
  executeManageAgentChanges: vi.fn(),
}));

vi.mock('@/workflows/skill-copy', () => ({
  executeSkillCopy: vi.fn(),
}));

vi.mock('../ManageAgentsDialog', () => ({
  ManageAgentsDialog: (props: {
    previewFailed?: boolean;
    onRetry?: () => void;
  }) => (
    <div data-testid="manage-container-dialog" data-preview-failed={String(props.previewFailed)}>
      <button type="button" onClick={props.onRetry}>retry</button>
    </div>
  ),
}));

vi.mock('../CopyToProjectDialog', () => ({
  CopyToProjectDialog: (props: {
    open: boolean;
    skill: InstalledSkill;
    sourceContext: ContextRef;
    onRepairSource?: (skill: InstalledSkill, context: ContextRef) => void;
  }) => (
    <div
      data-testid="copy-container-dialog"
      data-open={String(props.open)}
      data-skill={props.skill.name}
      data-project={props.sourceContext.scope.scope === 'project'
        ? props.sourceContext.scope.project_id
        : 'global'}
    >
      <button
        type="button"
        onClick={() => props.onRepairSource?.(props.skill, props.sourceContext)}
      >
        repair source
      </button>
    </div>
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

const context: ContextRef = {
  environment: { kind: 'host' },
  scope: { scope: 'project', project_id: 'source-project' },
};

const skill: InstalledSkill = {
  name: 'toolkit',
  description: '',
  path: '/project/.agents/skills/toolkit',
  canonicalPath: '/project/.agents/skills/toolkit',
  scope: 'project',
  agents: [],
  source: 'owner/repo',
};

describe('Skill dialog containers', () => {
  beforeEach(() => {
    mocks.openManageAgentChanges.mockReset();
    useSkillDialogStore.getState().closeManageAgents();
    useSkillDialogStore.getState().closeCopyToProject();
    useSkillDialogStore.getState().closeRepairSource();
    useSkillUpdateWorkflow.getState().reset();
  });

  it('keeps Manage Agents retry bound to the opened dialog session', () => {
    useSkillDialogStore.getState().openManageAgents(skill, context, '/project');
    useSkillDialogStore.getState().setManageAgentLoading(false);

    render(<ManageAgentsDialogContainer />);

    expect(screen.getByTestId('manage-container-dialog').dataset.previewFailed).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'retry' }));
    expect(mocks.openManageAgentChanges).toHaveBeenCalledWith(skill, context, '/project');
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

  it('keeps the Copy session mounted while source repair is open', () => {
    act(() => {
      useSkillDialogStore.getState().openCopyToProject(skill, context);
    });
    render(<CopyToProjectDialogContainer />);

    const dialog = screen.getByTestId('copy-container-dialog');
    expect(dialog.dataset.open).toBe('true');

    fireEvent.click(screen.getByRole('button', { name: 'repair source' }));

    expect(screen.getByTestId('copy-container-dialog')).toBe(dialog);
    expect(dialog.dataset.open).toBe('false');
    expect(useSkillDialogStore.getState().copySkill).toBe(skill);
    expect(useSkillDialogStore.getState().repairSourceTarget?.skillName).toBe('toolkit');

    act(() => {
      useSkillDialogStore.getState().closeRepairSource();
    });

    expect(screen.getByTestId('copy-container-dialog')).toBe(dialog);
    expect(dialog.dataset.open).toBe('true');
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
