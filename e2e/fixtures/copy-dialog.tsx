// 原生布局验收使用真实组件和受控数据，不调用复制或 WSL 写入接口。
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/manrope/600.css';
import i18n from '@/i18n';
import '@/index.css';
import { CopyToProjectDialog } from '@/components/skills/CopyToProjectDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { CopyAgentSelectionSnapshot, EnvironmentInfo, InstalledSkill, ProjectInfo } from '@/bindings';

const distro = 'Ubuntu-24.04-Development-Workspace';
const environments: EnvironmentInfo[] = [
  { environment: { kind: 'native' }, displayName: 'Windows', status: 'available', revision: 1, error: null },
  { environment: { kind: 'wsl', distro_name: distro }, displayName: distro, status: 'available', revision: 1, error: null },
];
const project = (id: string, displayName: string, nativePath: string): ProjectInfo => ({
  binding: { id, displayName, nativePath, order: null, suppressCrossStorageWarning: false },
  storage: { access: 'native', owner: null },
});
const projects = {
  native: [
    project('skills', 'skills', 'D:\\Code\\temp\\skills'),
    project('notes', 'notes', 'D:\\Code\\hccake\\notes'),
    project('grpc', 'grpc-demo', 'D:\\Code\\temp\\grpc-demo'),
    project('erp', 'hyldsys-erp', 'D:\\Code\\wb-projects\\hyldsys-erp'),
  ],
  [`wsl:${distro.toLowerCase()}`]: [
    project('linux-skills', 'Linux skills', '/home/developer/projects/skills'),
    project('linux-long', 'Long project name for layout verification', '/home/developer/workspaces/development/projects/long-project-directory'),
  ],
};
const skill: InstalledSkill = {
  name: 'antfu', description: 'Copy dialog layout fixture', scope: 'project',
  path: 'D:\\Code\\source\\.agents\\skills\\antfu', canonicalPath: 'D:\\Code\\source\\.agents\\skills\\antfu',
  agents: ['claude-code'], associatedAgents: ['claude-code'], source: 'antfu/skills',
};
const selection: CopyAgentSelectionSnapshot = {
  selection: {
    revision: 'copy-layout-v1', groups: [], unavailableExplicitAgents: [],
    baselineSelectedOptionIds: ['claude-code'], userModeOptionIds: ['claude-code', 'junie'],
    agents: [
      ...['Antigravity', 'Codex', 'Cursor', 'Gemini CLI', 'GitHub Copilot', 'OpenCode'].map(name => ({
        id: name.toLowerCase().replaceAll(' ', '-'), displayName: name, kind: 'standard' as const,
        detection: 'detected' as const, directoryAccess: 'standardOnly' as const, installOptionId: null, groupId: null,
      })),
      ...[['claude-code', 'Claude Code'], ['junie', 'Junie']].map(([id, name]) => ({
        id, displayName: name, kind: 'standard' as const, detection: 'detected' as const,
        directoryAccess: 'privateOnly' as const, installOptionId: id, groupId: null,
      })),
    ],
    installOptions: [['claude-code', 'Claude Code'], ['junie', 'Junie']].map(([id, name]) => ({
      id, displayName: name, kind: 'standardDirectory', agentIds: [id],
      path: `D:\\Code\\source\\.${id}\\skills`, groupId: null, selectable: true,
      modeConstraint: 'userSelectable', disabledReason: null,
    })),
  },
};
const loadAgentSelection = async () => selection;
const loadProjects = async () => {};
const checkExistence = async (_name: string, _environment: unknown, ids: string[]) => ids.map(id => ({ projectId: id, hasSkill: id !== 'erp' }));
type FixtureConfig = { language: 'zh-CN' | 'en'; dark: boolean; single: boolean; dependency: boolean };

function Fixture() {
  const [config, setConfig] = useState<FixtureConfig>({ language: 'zh-CN', dark: false, single: false, dependency: false });
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    Object.assign(window, { configureCopyDialog: async (next: FixtureConfig) => {
      await i18n.changeLanguage(next.language);
      document.documentElement.classList.toggle('dark', next.dark);
      setConfig(next);
      setGeneration(value => value + 1);
    } });
    return () => { delete (window as { configureCopyDialog?: unknown }).configureCopyDialog; };
  }, []);
  return <TooltipProvider><CopyToProjectDialog key={generation} skill={config.dependency ? {
    ...skill, dependency: { packageName: '@example/tools', recordedVersion: '2.0.0', declaredBy: '.' },
  } : skill} sourceContext={{ environment: { kind: 'native' }, scope: { scope: 'project', project_id: 'source' } }}
    environments={config.single ? environments.slice(0, 1) : environments} projectsByEnvironment={projects}
    loadAgentSelection={loadAgentSelection} onLoadProjects={loadProjects} checkExistence={checkExistence}
    onClose={() => {}} onCopy={async () => ({ status: 'blocked' })} /></TooltipProvider>;
}

void i18n.changeLanguage('zh-CN').then(() => createRoot(document.getElementById('root')!).render(<Fixture />));
