/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { RepairSourceDialog } from '../RepairSourceDialog';
import { useSkillDialogStore } from '@/stores/skill-dialog';
import type { FetchResult } from '@/bindings';
import { useMutationStore } from '@/stores/mutation';
import { toast } from 'sonner';

const mocks = vi.hoisted(() => ({
  fetchAvailable: vi.fn(),
  installSkills: vi.fn(),
  markSourceRepairSucceeded: vi.fn(),
  syncSkills: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (options?.name) return `${key}:${options.name}`;
      return key;
    },
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  fetchAvailable: (...args: unknown[]) => mocks.fetchAvailable(...args),
  installSkills: (...args: unknown[]) => mocks.installSkills(...args),
}));

vi.mock('sonner', () => ({
  toast: { error: vi.fn() },
}));

vi.mock('@/utils/cross-storage-guidance', () => ({
  appendCrossStorageFailureGuidance: (
    message: string,
    _context: unknown,
    operation: string,
  ) => `${message}\nGUIDANCE:${operation}`,
}));

vi.mock('@/stores/skills-data', () => ({
  useSkillsDataStore: (selector: (state: {
    markSourceRepairSucceeded: typeof mocks.markSourceRepairSucceeded;
    syncSkills: typeof mocks.syncSkills;
  }) => unknown) => selector({
    markSourceRepairSucceeded: mocks.markSourceRepairSucceeded,
    syncSkills: mocks.syncSkills,
  }),
}));

const fetchResult = (skillNames: string[], kind: 'none' | 'require-confirmation' = 'none'): FetchResult => ({
  sourceType: 'github',
  sourceUrl: 'https://github.com/owner/repo',
  gitRef: null,
  skillFilter: null,
  riskPolicy: { kind, code: kind === 'require-confirmation' ? 'openclaw' : null },
  skills: skillNames.map((name) => ({
    name,
    installDirName: name,
    description: name,
    relativePath: `skills/${name}`,
  })),
});

const hostGlobal = {
  environment: { kind: 'host' },
  scope: { scope: 'global' },
} as const;

describe('RepairSourceDialog', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.fetchAvailable.mockResolvedValue(fetchResult(['toolkit']));
    mocks.installSkills.mockResolvedValue({
      successful: [{
        skillName: 'toolkit',
        agent: 'claude-code',
        success: true,
        path: '/agent/toolkit',
        canonicalPath: '/canonical/toolkit',
        mode: 'copy',
        symlinkFailed: false,
        skipped: false,
        error: null,
      }],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useMutationStore.setState({ activeMutation: null, cancelling: false, loading: false });
    mocks.syncSkills.mockResolvedValue(undefined);
    useSkillDialogStore.setState({
      repairSourceTarget: null,
    });
  });

  it('disables repair but keeps source validation available during another mutation', () => {
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });
    useMutationStore.setState({
      activeMutation: {
        kind: 'update',
        context: hostGlobal,
        id: 'mutation-1',
        phase: 'preparing',
        progress: null,
        cancelable: true,
      },
    });

    render(<RepairSourceDialog />);

    expect((screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'skills.repairSourceDialog.validate' }) as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows a read-only source by default and repairs the current skill after source validation', async () => {
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    expect(screen.getByRole('textbox', { name: 'skills.repairSourceDialog.sourceLabel' })).toBeTruthy();
    expect(screen.queryByText('https://github.com/owner/repo#main')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.validate' }));

    await waitFor(() => {
      expect(screen.getByText('skills.repairSourceDialog.sourceContainsSkill:toolkit')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.objectContaining({
        source: 'https://github.com/owner/repo#main',
        skills: ['toolkit'],
        agents: ['claude-code'],
        scope: 'global',
        mode: 'copy',
        preserveExistingModes: true,
      }));
    });
    expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(hostGlobal, 'toolkit');
    expect(mocks.syncSkills).toHaveBeenCalledWith(hostGlobal);
  });

  it('inherits the target ContextRef for environment-aware repair', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'ubuntu-project' },
    } as const;
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'project',
        projectPath: '/work/app',
        agents: ['claude-code'],
        gitRef: 'main',
        context,
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.fetchAvailable).toHaveBeenCalledWith(
        context,
        'https://github.com/owner/repo#main',
      );
      expect(mocks.installSkills).toHaveBeenCalledWith(
        context,
        expect.objectContaining({ source: 'https://github.com/owner/repo#main' }),
      );
    });
  });

  it('does not clear repair state or close when install reports failures', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [],
      failed: [{
        skillName: 'toolkit',
        agent: 'claude-code',
        success: false,
        path: '/agent/toolkit',
        canonicalPath: null,
        mode: 'copy',
        symlinkFailed: false,
        skipped: false,
        error: 'copy failed',
      }],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.anything());
    });

    expect(mocks.markSourceRepairSucceeded).not.toHaveBeenCalled();
    expect(mocks.syncSkills).not.toHaveBeenCalled();
    expect(useSkillDialogStore.getState().repairSourceTarget?.skillName).toBe('toolkit');
  });

  it('does not clear repair state when install only reports skipped results', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [{
        skillName: 'toolkit',
        agent: 'claude-code',
        success: true,
        path: '/canonical/toolkit',
        canonicalPath: '/canonical/toolkit',
        mode: 'symlink',
        symlinkFailed: false,
        skipped: true,
        error: null,
      }],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.anything());
    });

    expect(mocks.markSourceRepairSucceeded).not.toHaveBeenCalled();
    expect(mocks.syncSkills).not.toHaveBeenCalled();
    expect(useSkillDialogStore.getState().repairSourceTarget?.skillName).toBe('toolkit');
  });

  it('adds storage-owner guidance when project source repair reports failures', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    mocks.installSkills.mockResolvedValueOnce({
      successful: [],
      failed: [{
        skillName: 'toolkit',
        agent: 'claude-code',
        success: false,
        path: '/mnt/c/Code/app/.agents/skills/toolkit',
        canonicalPath: null,
        mode: 'copy',
        symlinkFailed: false,
        skipped: false,
        error: 'permission denied',
      }],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'project',
        projectPath: '/mnt/c/Code/app',
        agents: ['claude-code'],
        gitRef: 'main',
        context,
      },
    });

    render(<RepairSourceDialog />);
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:repair'));
    });
  });

  it('adds storage-owner guidance when project source repair throws', async () => {
    const context = {
      environment: { kind: 'wsl', distro_name: 'Ubuntu' },
      scope: { scope: 'project', project_id: 'project-1' },
    } as const;
    mocks.installSkills.mockRejectedValueOnce(new Error('permission denied'));
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'project',
        projectPath: '/mnt/c/Code/app',
        agents: ['claude-code'],
        gitRef: 'main',
        context,
      },
    });

    render(<RepairSourceDialog />);
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('GUIDANCE:repair'));
    });
  });

  it('does not treat duplicate successful agent results as complete repair', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [
        {
          skillName: 'toolkit',
          agent: 'claude-code',
          success: true,
          path: '/agent/toolkit',
          canonicalPath: '/canonical/toolkit',
          mode: 'copy',
          symlinkFailed: false,
          skipped: false,
          error: null,
        },
        {
          skillName: 'toolkit',
          agent: 'claude-code',
          success: true,
          path: '/agent/toolkit-duplicate',
          canonicalPath: '/canonical/toolkit',
          mode: 'copy',
          symlinkFailed: false,
          skipped: false,
          error: null,
        },
      ],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code', 'cursor'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.anything());
    });

    expect(mocks.markSourceRepairSucceeded).not.toHaveBeenCalled();
    expect(mocks.syncSkills).not.toHaveBeenCalled();
    expect(useSkillDialogStore.getState().repairSourceTarget?.skillName).toBe('toolkit');
  });

  it('deduplicates target agents before repairing', async () => {
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code', 'claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.objectContaining({
        agents: ['claude-code'],
      }));
    });
  });

  it('repairs canonical only when the skill is only default available', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [{
        skillName: 'toolkit',
        agent: '__canonical__',
        success: true,
        path: '/canonical/toolkit',
        canonicalPath: '/canonical/toolkit',
        mode: 'copy',
        symlinkFailed: false,
        skipped: false,
        error: null,
      }],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['codex', 'firebender'],
        defaultAvailableAgents: ['codex', 'firebender'],
        privateAdaptedAgents: [],
        privateCopyAgents: [],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.objectContaining({
        agents: [],
        privateCopyAgents: [],
        retry: true,
      }));
    });
    expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(hostGlobal, 'toolkit');
  });

  it('repairs private adapted and existing independent-copy targets separately', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [
        {
          skillName: 'toolkit',
          agent: 'cursor',
          success: true,
          path: '/cursor/toolkit',
          canonicalPath: '/canonical/toolkit',
          mode: 'copy',
          symlinkFailed: false,
          skipped: false,
          error: null,
        },
        {
          skillName: 'toolkit',
          agent: 'firebender',
          success: true,
          path: '/firebender/toolkit',
          canonicalPath: '/canonical/toolkit',
          mode: 'copy',
          symlinkFailed: false,
          skipped: false,
          error: null,
        },
      ],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['codex', 'cursor', 'firebender'],
        defaultAvailableAgents: ['codex', 'firebender'],
        privateAdaptedAgents: ['cursor'],
        privateCopyAgents: ['firebender'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.objectContaining({
        agents: ['cursor'],
        privateCopyAgents: ['firebender'],
        retry: true,
      }));
    });
    expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(hostGlobal, 'toolkit');
  });

  it('treats canonical-only repair as successful when no target agents are associated', async () => {
    mocks.installSkills.mockResolvedValue({
      successful: [{
        skillName: 'toolkit',
        agent: '__canonical__',
        success: true,
        path: '/canonical/toolkit',
        canonicalPath: '/canonical/toolkit',
        mode: 'copy',
        symlinkFailed: false,
        skipped: false,
        error: null,
      }],
      failed: [],
      symlinkFallbackAgents: [],
    });
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: [],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.markSourceRepairSucceeded).toHaveBeenCalledWith(hostGlobal, 'toolkit');
    });
    expect(mocks.syncSkills).toHaveBeenCalledWith(hostGlobal);
  });

  it('renders repair context as compact summary rows', () => {
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code', 'codex'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    expect(screen.getByText('skills.repairSourceDialog.overwriteNotice')).toBeTruthy();
  });

  it('allows changing source and blocks repair when the source does not contain the current skill', async () => {
    mocks.fetchAvailable.mockResolvedValue(fetchResult(['other-skill']));
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.change(screen.getByRole('textbox', { name: 'skills.repairSourceDialog.sourceLabel' }), {
      target: { value: 'https://github.com/owner/other' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.validate' }));

    await waitFor(() => {
      expect(screen.getByText('skills.repairSourceDialog.sourceMissingSkill:toolkit')).toBeTruthy();
    });

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.repair' }).disabled).toBe(true);
    expect(mocks.installSkills).not.toHaveBeenCalled();
  });

  it('requires explicit risk acknowledgement before repairing risky sources', async () => {
    mocks.fetchAvailable.mockResolvedValue(fetchResult(['toolkit'], 'require-confirmation'));
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'openclaw/community-skills',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: null,
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.validate' }));

    await waitFor(() => {
      expect(screen.getByText('addSkill.risk.openclawAcknowledge')).toBeTruthy();
    });

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.repair' }).disabled).toBe(true);

    fireEvent.click(screen.getByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.objectContaining({
        acknowledgeRisk: true,
      }));
    });
  });

  it('resets risk acknowledgement when the source changes', async () => {
    mocks.fetchAvailable.mockResolvedValue(fetchResult(['toolkit'], 'require-confirmation'));
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'openclaw/community-skills',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: null,
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.validate' }));

    await waitFor(() => {
      expect(screen.getByText('addSkill.risk.openclawAcknowledge')).toBeTruthy();
    });

    fireEvent.click(screen.getByRole('checkbox'));
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.repair' }).disabled).toBe(false);

    fireEvent.change(screen.getByRole('textbox', { name: 'skills.repairSourceDialog.sourceLabel' }), {
      target: { value: 'openclaw/other-skills' },
    });

    expect(screen.queryByText('addSkill.risk.openclawAcknowledge')).toBeNull();
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.repair' }).disabled).toBe(false);
  });

  it('keeps repair as the only loading action while repair validates and installs', async () => {
    let resolveFetch: (value: FetchResult) => void = () => {};
    let resolveInstall: () => void = () => {};
    mocks.fetchAvailable.mockReturnValue(new Promise<FetchResult>((resolve) => {
      resolveFetch = resolve;
    }));
    mocks.installSkills.mockReturnValue(new Promise((resolve) => {
      resolveInstall = () => resolve({
        successful: [{
          skillName: 'toolkit',
          agent: 'claude-code',
          success: true,
          path: '/agent/toolkit',
          canonicalPath: '/canonical/toolkit',
          mode: 'copy',
          symlinkFailed: false,
          skipped: false,
          error: null,
        }],
        failed: [],
        symlinkFallbackAgents: [],
      });
    }));
    useSkillDialogStore.setState({
      repairSourceTarget: {
        skillName: 'toolkit',
        source: 'https://github.com/owner/repo#main',
        scope: 'global',
        context: hostGlobal,
        agents: ['claude-code'],
        gitRef: 'main',
      },
    });

    render(<RepairSourceDialog />);

    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.repair' }));
    fireEvent.click(screen.getByRole('button', { name: 'skills.repairSourceDialog.validating' }));

    expect(screen.getByRole<HTMLInputElement>('textbox', { name: 'skills.repairSourceDialog.sourceLabel' }).disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.validate' }).disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.validating' }).disabled).toBe(true);
    expect(mocks.fetchAvailable).toHaveBeenCalledTimes(1);
    expect(mocks.fetchAvailable).toHaveBeenCalledWith(
      hostGlobal,
      'https://github.com/owner/repo#main',
    );

    resolveFetch(fetchResult(['toolkit']));

    await waitFor(() => {
      expect(mocks.installSkills).toHaveBeenCalledTimes(1);
      expect(mocks.installSkills).toHaveBeenCalledWith(hostGlobal, expect.anything());
    });

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'skills.repairSourceDialog.repairing' }).disabled).toBe(true);

    resolveInstall();

    await waitFor(() => {
      expect(mocks.syncSkills).toHaveBeenCalledWith(hostGlobal);
    });
  });
});
