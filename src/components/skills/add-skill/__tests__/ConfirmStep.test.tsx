/* @vitest-environment jsdom */

import '@/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { checkOverwrites, checkSkillAudit } from '@/hooks/useTauriApi';
import type { WizardState } from '../types';
import { ConfirmStep } from '../ConfirmStep';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      if (key === 'addSkill.confirm.summary') {
        return `Will install ${options?.count} skills. ${options?.overwriteCount} will overwrite existing content.`;
      }
      if (key === 'addSkill.confirm.summaryNoOverwrite') {
        return `Will install ${options?.count} skills.`;
      }
      if (key === 'addSkill.confirm.itemsTitle') return 'Install contents';
      if (key === 'addSkill.confirm.overwriteGroup') return '覆盖安装';
      if (key === 'addSkill.confirm.directories') return 'Install locations';
      if (key === 'addSkill.confirm.directoryHint') {
        return 'Skills are installed to the shared directory. Additional agents are handled using the selected method.';
      }
      if (key === 'addSkill.confirm.shared') return 'Shared directory';
      if (key === 'addSkill.confirm.symlinkHint') {
        return 'Create symlinks from additional agents to the shared directory.';
      }
      if (key === 'addSkill.confirm.copyHint') {
        return 'Copy the Skill into each additional agent directory.';
      }
      return key;
    },
  }),
}));

vi.mock('@/hooks/useTauriApi', () => ({
  checkOverwrites: vi.fn().mockResolvedValue({}),
  checkSkillAudit: vi.fn().mockResolvedValue(null),
}));

const checkOverwritesMock = vi.mocked(checkOverwrites);
const checkSkillAuditMock = vi.mocked(checkSkillAudit);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function createState(): WizardState {
  return {
    step: 'confirm',
    entryPoint: 'skills-panel',
    scope: 'global',
    projectPath: undefined,
    source: 'openclaw/community-skills',
    fetchStatus: 'success',
    fetchError: null,
    gitRef: null,
    availableSkills: [{ name: 'demo', description: 'Demo', relativePath: 'skills/demo/SKILL.md', pluginName: null }],
    selectedSkills: ['demo'],
    skillFilter: null,
    skillSearchQuery: '',
    selectedAgents: ['codex'],
    allAgents: [],
    mode: 'symlink',
    otherAgentsExpanded: false,
    otherAgentsSearchQuery: '',
    overwrites: {},
    confirmReady: true,
    preSelectedSkills: [],
    preSelectedAgents: [],
    installResults: null,
    installError: undefined,
    retrySkillName: undefined,
    retryAgents: undefined,
    riskPolicy: { kind: 'require-confirmation', code: 'openclaw' },
    riskAcknowledged: false,
  };
}

function createTrustState(
  trustFields: Record<string, unknown>,
  source = 'https://example.com'
): WizardState {
  return {
    ...createState(),
    source,
    riskPolicy: { kind: 'none', code: null },
    availableSkills: [
      {
        name: 'demo',
        description: 'Demo',
        relativePath: 'demo/SKILL.md',
        pluginName: null,
        ...trustFields,
      } as never,
    ],
  };
}

describe('ConfirmStep', () => {
  beforeEach(() => {
    checkOverwritesMock.mockReset();
    checkOverwritesMock.mockResolvedValue({});
    checkSkillAuditMock.mockReset();
    checkSkillAuditMock.mockResolvedValue(null);
  });

  it('checks overwrites for automatic agents when only the shared directory will be used', async () => {
    const updateState = vi.fn();

    render(
      <ConfirmStep
        state={{
          ...createState(),
          selectedAgents: [],
          allAgents: [{
            id: 'warp',
            name: 'Warp',
            skillsDir: '.agents/skills',
            globalSkillsDir: '~/.agents/skills',
            detected: true,
            targets: {
              global: { supported: true, automatic: true, path: '~/.agents/skills' },
              project: { supported: true, automatic: true, path: '.agents/skills' },
            },
          }],
          confirmReady: false,
          riskPolicy: { kind: 'none', code: null },
        }}
        updateState={updateState}
        scope="global"
      />
    );

    await waitFor(() => {
      expect(updateState).toHaveBeenCalledWith({ overwrites: {}, confirmReady: true });
    });
    expect(checkOverwritesMock).toHaveBeenCalledWith(
      ['demo'],
      ['warp'],
      'global',
      undefined
    );
    expect(checkSkillAuditMock).toHaveBeenCalledWith('openclaw/community-skills', ['demo']);
  });

  it('ignores stale overwrite results from an older confirmation request', async () => {
    const firstOverwrite = deferred<Record<string, string[]>>();
    const secondOverwrite = deferred<Record<string, string[]>>();
    const updateState = vi.fn();

    checkOverwritesMock
      .mockReturnValueOnce(firstOverwrite.promise)
      .mockReturnValueOnce(secondOverwrite.promise);

    const { rerender } = render(
      <ConfirmStep
        state={{
          ...createState(),
          selectedSkills: ['first-skill'],
          confirmReady: false,
          riskPolicy: { kind: 'none', code: null },
        }}
        updateState={updateState}
        scope="global"
      />
    );

    rerender(
      <ConfirmStep
        state={{
          ...createState(),
          selectedSkills: ['second-skill'],
          confirmReady: false,
          riskPolicy: { kind: 'none', code: null },
        }}
        updateState={updateState}
        scope="global"
      />
    );

    secondOverwrite.resolve({ 'second-skill': ['Codex'] });

    await waitFor(() => {
      expect(updateState).toHaveBeenCalledWith({
        overwrites: { 'second-skill': ['Codex'] },
        confirmReady: true,
      });
    });

    firstOverwrite.resolve({ 'first-skill': ['Cursor'] });

    await waitFor(() => {
      expect(updateState).not.toHaveBeenCalledWith({
        overwrites: { 'first-skill': ['Cursor'] },
        confirmReady: true,
      });
    });
  });

  it('summarizes the install plan and uses overwrite-install status only for conflicted skills', () => {
    render(
      <ConfirmStep
        state={{
          ...createState(),
          riskPolicy: { kind: 'none', code: null },
          availableSkills: [
            { name: 'existing-skill', description: 'Existing', relativePath: 'skills/existing/SKILL.md', pluginName: null },
            { name: 'new-skill', description: 'New', relativePath: 'skills/new/SKILL.md', pluginName: null },
          ],
          selectedSkills: ['existing-skill', 'new-skill'],
          overwrites: { 'existing-skill': ['Claude Code'] },
        }}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Will install 2 skills. 1 will overwrite existing content.')).toBeTruthy();
    expect(screen.getByText('Install contents')).toBeTruthy();
    const installContentsSection = screen.getByText('Install contents').closest('div');
    expect(installContentsSection?.textContent).toContain('Will install 2 skills. 1 will overwrite existing content.');
    expect(screen.getByText('覆盖安装')).toBeTruthy();
    expect(screen.queryByText('将新增')).toBeNull();
    expect(screen.queryByText('1 locations')).toBeNull();
  });

  it('does not show overwrite agent details in the summary list', async () => {
    render(
      <ConfirmStep
        state={{
          ...createState(),
          riskPolicy: { kind: 'none', code: null },
          availableSkills: [
            { name: 'existing-skill', description: 'Existing', relativePath: 'skills/existing/SKILL.md', pluginName: null },
          ],
          selectedSkills: ['existing-skill'],
          overwrites: { 'existing-skill': ['Claude Code', 'Codex', 'Cursor'] },
        }}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.queryByText('Claude Code')).toBeNull();
    expect(screen.queryByText('Codex')).toBeNull();
  });

  it('uses install location language for the shared directory topology', () => {
    render(
      <ConfirmStep
        state={{
          ...createState(),
          riskPolicy: { kind: 'none', code: null },
        }}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('Install locations')).toBeTruthy();
    expect(screen.getByText('Shared directory')).toBeTruthy();
    expect(screen.getByText('~/.agents/skills')).toBeTruthy();
    expect(screen.getByText(/Skills are installed to the shared directory/)).toBeTruthy();
  });

  it('renders guarded-source risk confirmation UI', () => {
    render(
      <ConfirmStep
        state={createState()}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.risk.openclawTitle')).toBeTruthy();
    expect(screen.getByText('addSkill.risk.openclawAcknowledge')).toBeTruthy();
  });

  it('toggles riskAcknowledged when the shadcn checkbox is clicked', async () => {
    const updateState = vi.fn();
    render(
      <ConfirmStep
        state={createState()}
        updateState={updateState}
        scope="global"
      />
    );

    const checkbox = screen.getByRole('checkbox');
    await userEvent.click(checkbox);

    expect(updateState).toHaveBeenCalledWith({ riskAcknowledged: true });
  });

  it('shows legacy well-known trust metadata without digest verification', () => {
    render(
      <ConfirmStep
        state={createTrustState({
          wellKnownVersion: '0.1.0',
          wellKnownEntryType: 'legacy',
          trustReason: 'legacy',
        })}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.confirm.trust.legacy')).toBeTruthy();
    expect(screen.queryByText('addSkill.confirm.trust.digestVerified')).toBeNull();
  });

  it('shows v2 well-known artifact host, type, and digest verification', () => {
    render(
      <ConfirmStep
        state={createTrustState({
          wellKnownVersion: '0.2.0',
          wellKnownEntryType: 'skill-md',
          artifactUrlHost: 'assets.example.com',
          digestVerified: true,
          trustReason: 'digest-verified',
        })}
        updateState={vi.fn()}
        scope="global"
      />
    );

    expect(screen.getByText('addSkill.confirm.trust.skillMd')).toBeTruthy();
    expect(screen.getByText('assets.example.com')).toBeTruthy();
    expect(screen.getByText('addSkill.confirm.trust.digestVerified')).toBeTruthy();
  });
});
