import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ContextRef, InstalledSkill } from '@/bindings';
import { contextKey, globalContext } from '@/lib/context';
import { useProjectStore } from '../projects';
import { useSkillsDataStore } from '../skills-data';
import { useWorkspaceContextStore } from '../workspace-context';
import { useSkillDetailStore } from '../skill-detail';

const mocks = vi.hoisted(() => ({
  readSkillContent: vi.fn(),
}));

vi.mock('@/hooks/useTauriApi', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useTauriApi')>();
  return {
    ...actual,
    readSkillContent: (...args: unknown[]) => mocks.readSkillContent(...args),
  };
});

const projectContext: ContextRef = {
  environment: { kind: 'wsl', distro_name: 'Ubuntu' },
  scope: { scope: 'project', project_id: 'project-a' },
};

const toolkit: InstalledSkill = {
  name: 'toolkit',
  description: '',
  path: '/skills/toolkit',
  canonicalPath: '/canonical/toolkit',
  scope: 'project',
  agents: [],
  hasUpdate: false,
};

const globalToolkit: InstalledSkill = {
  ...toolkit,
  path: '/global-skills/toolkit',
  canonicalPath: '/global-canonical/toolkit',
  scope: 'global',
};

describe('Skill detail workspace context', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useWorkspaceContextStore.setState({ selectedContext: projectContext });
    useProjectStore.setState({
      projectsByEnvironment: {
        'wsl:ubuntu': [{
          binding: {
            id: 'project-a',
            nativePath: '/home/me/project-a',
            displayName: null,
            order: null,
            suppressCrossStorageWarning: false,
          },
          storage: { access: 'native', owner: projectContext.environment },
        }],
      },
    });
    useSkillsDataStore.setState({
      snapshots: {
        [contextKey(globalContext(projectContext.environment))]: {
          skills: [globalToolkit],
          agents: [],
          pathExists: true,
          loading: false,
          error: null,
          requestId: 1,
        },
        [contextKey(projectContext)]: {
          skills: [toolkit],
          agents: [],
          pathExists: true,
          loading: false,
          error: null,
          requestId: 1,
        },
      },
    });
    useSkillDetailStore.setState({
      selectedSkillRef: null,
      selectedContext: null,
      skillContent: null,
      loadingContent: false,
    });
    mocks.readSkillContent.mockResolvedValue('# Toolkit');
  });

  it('derives project identity and reloads content from the committed context snapshot', async () => {
    await useSkillDetailStore.getState().selectSkill(toolkit);
    await useSkillDetailStore.getState().reloadContent();

    expect(useSkillDetailStore.getState().selectedSkillRef).toEqual({
      name: 'toolkit',
      scope: 'project',
      projectPath: '/home/me/project-a',
    });
    expect(mocks.readSkillContent).toHaveBeenLastCalledWith(
      { context: projectContext, skillName: 'toolkit' },
    );
  });

  it('reads a global Skill from the Environment global Context while a project is selected', async () => {
    await useSkillDetailStore.getState().selectSkill(globalToolkit);

    const expectedContext = globalContext(projectContext.environment);
    expect(useSkillDetailStore.getState().selectedContext).toEqual(expectedContext);
    expect(mocks.readSkillContent).toHaveBeenCalledWith({
      context: expectedContext,
      skillName: 'toolkit',
    });
  });

  it('reloads a global Skill from its captured global Context after the workspace changes', async () => {
    await useSkillDetailStore.getState().selectSkill(globalToolkit);
    mocks.readSkillContent.mockClear();
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: { kind: 'host' },
        scope: { scope: 'global' },
      },
    });

    await useSkillDetailStore.getState().reloadContent();

    expect(mocks.readSkillContent).toHaveBeenCalledWith({
      context: globalContext(projectContext.environment),
      skillName: 'toolkit',
    });
  });

  it('reloads from the selection context after the workspace changes', async () => {
    await useSkillDetailStore.getState().selectSkill(toolkit);
    mocks.readSkillContent.mockClear();
    useWorkspaceContextStore.setState({
      selectedContext: {
        environment: { kind: 'host' },
        scope: { scope: 'global' },
      },
    });

    await useSkillDetailStore.getState().reloadContent();

    expect(mocks.readSkillContent).toHaveBeenCalledWith(
      { context: projectContext, skillName: 'toolkit' },
    );
  });
});
