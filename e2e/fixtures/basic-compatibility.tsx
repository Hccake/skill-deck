import { WindowLifecycleContext } from '@/lifecycle/useWindowLifecycle';
import { WizardPage } from '@/pages/WizardPage';
import { LibraryAddDialog } from '@/components/library/LibraryAddDialog';
import { commands, type FetchResult } from '@/bindings';
import { createRoot } from 'react-dom/client';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { MemoryRouter } from 'react-router-dom';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/manrope/600.css';
import '@/index.css';
import i18n from '@/i18n';
import { Header } from '@/components/layout/Header';
import { SkillsPage } from '@/pages/SkillsPage';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useSkillDetailStore } from '@/stores/skill-detail';
import { useInstallWizardSessionStore } from '@/stores/install-wizard-session';
import { useTranslationSettings } from '@/stores/translation';
import { useSettingsStore, applyPersistedAppearance } from '@/stores/settings';
import { useRecoveryStore } from '@/stores/recovery';
import { projectWorkspace } from '@/stores/projects';
import { getSkillIdentity } from '@/lib/skills/identity';
import type { InstalledSkill, ResolvedAgent, ProjectInfo } from '@/bindings';

// 原生界面验收使用实际页面组件，业务数据与写入入口限定在本夹具内。
const lang = 'zh-CN';
const windows = navigator.userAgent.includes('Windows');
const separator = windows ? '\\' : '/';
const at = (root: string, ...parts: string[]) => [root, ...parts].join(separator);
await i18n.changeLanguage(lang);
const native = { kind: 'native' as const };
const context = { environment: native, scope: { scope: 'project' as const, project_id: 'portal' } };
const projectPath = windows ? 'C:\\Projects\\customer-portal' : '/work/customer-portal';
const homePath = windows ? 'C:\\Users\\demo' : '/home/demo';
const secondProjectPath = windows ? 'C:\\Projects\\design-system' : '/work/design-system';
const projects: ProjectInfo[] = [
  { binding: { id: 'portal', nativePath: projectPath, displayName: 'customer-portal', order: 0 }, storage: { access: 'native', owner: native } },
  { binding: { id: 'design', nativePath: secondProjectPath, displayName: 'design-system', order: 1 }, storage: { access: 'native', owner: native } },
];
const projectSnapshot = { environment: native, phase: 'ready', projects, error: null, completeness: 'complete', environmentRevision: 1, lastAttemptAt: null, lastSuccessAt: 1, freshUntil: null, version: 1 };
Object.assign(projectWorkspace, { getSnapshot: () => projectSnapshot, subscribe: () => () => {}, execute: async () => ({ status: 'succeeded', snapshot: projectSnapshot }) });
useWorkspaceContextStore.setState({ selectedContext: context, selectGlobal: async () => useWorkspaceContextStore.setState({ selectedContext: { environment: native, scope: { scope: 'global' } } }), selectProject: async (id) => useWorkspaceContextStore.setState({ selectedContext: { environment: native, scope: { scope: 'project', project_id: id } } }) });
useInstallWizardSessionStore.setState({ active: false, loading: false, syncError: null });
useRecoveryStore.setState({ load: async () => {}, resources: [], state: 'ready', error: null });
useTranslationSettings.setState({ load: async () => {}, settings: { revision: 'review', serviceRevision: 'review', service: { kind: 'google' }, profiles: [], autoTranslate: false, noticeAccepted: true } });
useSettingsStore.setState({ locale: lang as 'zh-CN', theme: 'light' });
const readScope = { enabled: true, readsStandard: true, standardPath: at(projectPath, '.agents', 'skills'), privatePath: null, readPaths: [], standardPresence: 'present' as const, privatePresence: null, legacyPaths: [] };
const agents: ResolvedAgent[] = [['codex', 'Codex'], ['claude-code', 'Claude Code']].map(([id, displayName]) => ({
  definition: { id, displayName, source: 'builtin', aliases: [], global: { enabled: true, readsStandard: true, privatePath: null }, project: { enabled: true, readsStandard: true, privatePath: null }, detection: { kind: 'anyPathExists', paths: [] }, legacyPaths: [], adapter: 'standard' },
  detection: 'detected', detectionReason: null, global: { ...readScope, standardPath: at(homePath, '.agents', 'skills') }, project: readScope,
}));
const skill = (name: string, description: string, extra = {}): InstalledSkill => ({
  name, description, scope: 'project', path: at(projectPath, '.agents', 'skills', name), canonicalPath: at(projectPath, '.agents', 'skills', name),
  agents: ['codex', 'claude-code'], associatedAgents: ['codex', 'claude-code'],
  source: 'acme/team-skills', sourceUrl: 'https://github.com/acme/team-skills', sourceType: 'github',
  installedAt: '2026-10-21T09:00:00+08:00', updatedAt: '2026-11-05T16:20:00+08:00', canCheckForUpdates: true, canRunUpdate: true,
  sourceFilter: { key: 'git', kind: 'git', label: 'acme/team-skills', detail: 'github.com' }, ...extra,
} as InstalledSkill);
const downloads = skill('code-review', '根据项目规范审查代码变更，重点检查行为边界、可维护性与测试覆盖。', {
  source: 'docs.example.com', sourceUrl: 'https://docs.example.com/skills/code-review.zip', sourceType: 'download',
  canCheckForUpdates: false, canRunUpdate: false, updateReason: 'download-source',
  sourceFilter: { key: 'download', kind: 'download', label: 'docs.example.com', detail: 'https://docs.example.com' },
});
const projectSkills = [downloads,
  skill('frontend-design', '构建清晰、一致且易于使用的前端界面。', { gitRef: 'release/2.0', refKind: 'branch', hasUpdate: true, updateStatus: 'updateAvailable' }),
  skill('release-check', '检查发布内容、版本号和交付文件。', { gitRef: '0df84aa016957a2d18060649f3fcac9746abcde0', refKind: 'commit' }),
  skill('project-conventions', '项目约定和本地开发指南。', { source: null, sourceUrl: null, sourceType: 'local', canCheckForUpdates: false, canRunUpdate: false, updateReason: 'local-source', sourceFilter: { key: 'local', kind: 'local', label: '本地 Skill', detail: '' } }),
];
const globalSkills = [skill('writing-plans', '将已确认的设计拆分为可验证的实施步骤。', { scope: 'global', path: at(homePath, '.agents', 'skills', 'writing-plans'), canonicalPath: at(homePath, '.agents', 'skills', 'writing-plans') })];
const emptyLibrary = { orderedLibraries: [], selectedAgentIds: [], pending: false, syncState: 'synced' as const };
const snapshot = (skills: InstalledSkill[], root: string) => ({ skills, agents, libraryApplication: emptyLibrary, loading: false, error: null, requestId: 1, pathExists: true, pathBase: { logicalRoot: { environment: native, nativePath: root }, physicalRoot: { environment: native, nativePath: root }, pathStyle: windows ? 'windows' as const : 'posix' as const } });
useSkillsDataStore.setState({ snapshots: { 'native/global': snapshot(globalSkills, homePath), 'native/project:portal': snapshot(projectSkills, projectPath), 'native/project:design': snapshot([], secondProjectPath) }, refreshWorkspace: async () => {}, activateAutomaticChecks: async () => {}, forceCheckUpdates: async () => 'completed', syncSkills: async () => {} });
const content = `## 代码审查\n\n阅读相关变更及其调用方，根据实际影响给出审查意见。\n\n### 检查重点\n\n- 行为是否符合项目约定。\n- 错误处理是否保留上下文。\n- 测试是否覆盖关键边界。\n\n### 输出方式\n\n优先说明会影响用户的具体问题，并附上对应位置和建议。`;
useSkillDetailStore.setState({ detailOpen: true, selectedSkillRef: getSkillIdentity(downloads, projectPath), selectedContext: context, skillContent: content, loadingContent: false, contentError: false,
  selectSkill: async (selected) => useSkillDetailStore.setState({ detailOpen: true, selectedSkillRef: getSkillIdentity(selected, selected.scope === 'project' ? projectPath : undefined), skillContent: content }), reloadContent: async () => {},
});
Object.assign(window, { configureBasicCompatibility: async (options: {language: 'zh-CN' | 'en'; dark: boolean; detail: boolean}) => {
  await getCurrentWindow().setFocus();
  await i18n.changeLanguage(options.language);
  applyPersistedAppearance(options.dark ? 'dark' : 'light');
  useSettingsStore.setState({locale: options.language, theme: options.dark ? 'dark' : 'light'});
  useSkillDetailStore.setState({detailOpen: options.detail, selectedSkillRef: getSkillIdentity(downloads, projectPath), selectedContext: context, skillContent: content});
} });
function Fixture() {
  return <MemoryRouter><TooltipProvider><div className="flex h-screen min-h-0 flex-col bg-background text-foreground"><Header /><main className="min-h-0 flex-1"><SkillsPage /></main></div></TooltipProvider></MemoryRouter>;
}
const root = createRoot(document.getElementById('root')!);
const selectionFixture: FetchResult = {
  discoverySession: { sessionId: 'fixture', environment: native, sourceFingerprint: 'fixture', expiresAtEpochMs: Date.now() + 60_000 },
  sourceType: 'github', sourceUrl: 'https://github.com/acme/team-skills', gitRef: 'v1.2.3', skillFilter: 'code-review',
  skills: [
    { name: 'Code Review', installDirName: 'code-review', description: 'Review the shared project conventions.', relativePath: 'skills/shared/project-conventions/code-review/SKILL.md' },
    { name: 'code_review', installDirName: 'code-review', description: 'Review the application code and automated checks.', relativePath: 'packages/customer-portal/engineering/quality/checks/code-review/SKILL.md' },
  ],
  selectionResult: { matchedNames: [], unmatchedInputs: ['missing-skill'], ambiguousInputs: [{ input: 'code-review', candidateNames: ['Code Review', 'code_review'] }] },
};
commands.discoverSkillSource = async () => ({ status: 'ok', data: selectionFixture });
Object.assign(window, { inspectReasonix: async () => {
  const context = { environment: native, scope: { scope: 'global' as const } };
  const settings = await commands.getAgentSettingsSnapshot(context);
  const runtime = await commands.listAgents(context);
  if (runtime.status === 'error') throw runtime.error;
  return { rule: settings.activeBuiltin.find(agent => agent.id === 'reasonix'), resolved: runtime.data.agents.reasonix };
}, setCompatibilityFlow: (mode: 'skills' | 'wizard' | 'library') => {
  root.render(mode === 'skills' ? <Fixture /> : <MemoryRouter key={mode} initialEntries={['/?prefillSource=acme%2Fteam-skills%23%40code-review']}><TooltipProvider><WindowLifecycleContext.Provider value={{ requestAction: async () => {} }}>
    {mode === 'wizard' ? <WizardPage /> : <><div className="h-screen bg-background" /><LibraryAddDialog open target={{ environment: native, environmentName: 'Windows', libraryId: 'fixture', libraryName: 'Team Skills' }} existingSkillNames={new Set()} execute={async () => { throw new Error('UI fixture cannot write'); }} onClose={() => {}} /></>}
  </WindowLifecycleContext.Provider></TooltipProvider></MemoryRouter>);
} });
root.render(<Fixture />);
