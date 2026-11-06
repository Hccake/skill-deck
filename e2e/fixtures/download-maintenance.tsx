// 使用真实向导和更新弹窗验证布局；来源、选择及写入命令均为本夹具的数据。
import { createRoot } from 'react-dom/client';
import { MemoryRouter } from 'react-router-dom';
import { getCurrentWindow } from '@tauri-apps/api/window';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/manrope/600.css';
import '@/index.css';
import i18n from '@/i18n';
import { commands, type AgentSelectionSnapshot } from '@/bindings';
import type { WizardState } from '@/components/skills/add-skill/types';
import { WizardPage } from '@/pages/WizardPage';
import { UpdatePlanDialog } from '@/components/skills/UpdatePlanDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { WindowLifecycleContext } from '@/lifecycle/useWindowLifecycle';
import { applyPersistedAppearance, useSettingsStore } from '@/stores/settings';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import { useMutationStore } from '@/stores/mutation';

type Config = { language: 'zh-CN' | 'en'; dark: boolean; count: number; long: boolean; redirect: boolean; oneTime: boolean; view: 'wizard' | 'update'; step: 'source' | 'skills' | 'options' | 'confirm'; conflict: boolean };
const initial: Config = { language: 'zh-CN', dark: false, count: 20, long: false, redirect: true, oneTime: false, view: 'wizard', step: 'confirm', conflict: false };
const native = { kind: 'native' as const };
const context = { environment: native, scope: { scope: 'project' as const, project_id: 'fixture' } };
const windows = navigator.userAgent.includes('Windows');
const projectPath = windows ? 'C:\\Projects\\customer-portal' : '/work/customer-portal';
const root = createRoot(document.getElementById('root')!);
const sharedNames = ['Codex', 'Gemini CLI', 'OpenCode', 'Cursor', 'Windsurf', 'Cline', 'Roo Code', 'Kilo Code', 'Amp', 'Goose', 'Aider', 'Team Assistant'];
let config = initial;
let selection: AgentSelectionSnapshot;
let wizardState: WizardState;
let renderId = 0;
const fixture = { wizardState: null as unknown as WizardState, lastPrepared: [] as string[] };
Object.assign(window, { __downloadFixture: fixture });
commands.getInstallAgentSelection = async () => ({ status: 'ok', data: { selection, selectionHistoryWarning: null } });
commands.confirmInstallAgentSelection = async () => ({ status: 'ok', data: { status: 'ready', warning: null } });
commands.acquireSelectedPayloads = async request => ({ status: 'ok', data: request.skillPaths.map((skillPath, index) => ({ sessionId: 'fixture', skillPath, environment: native, payloadId: `fixture-${index}`, manifestHash: 'fixture', sourceFingerprint: 'fixture', expiresAtEpochMs: Date.now() + 60_000 })) });
commands.previewInstall = async request => {
  fixture.lastPrepared = [...request.skills];
  return { status: 'ok', data: { status: 'ready', preview: {
    token: { generation: 'fixture', registryRevision: 'fixture', environmentRevision: 'fixture', contextRevision: 'fixture' },
    skills: request.skills.map((skillName, index) => ({ skillName, overwriteTargets: [], blockingReasons: config.conflict && index === request.skills.length - 1 ? ['directDownloadConflict'] : [], fallbackForecasts: [], overridesLibrary: false, replacesDependency: false })),
  } } };
};
commands.discoverSkillSource = async () => ({ status: 'ok', data: { discoverySession: wizardState.discoverySession!, sourceType: 'download', sourceUrl: wizardState.source, skills: wizardState.availableSkills, gitRef: null, skillFilter: null, redirectedDownloadHost: wizardState.redirectedDownloadHost } });
commands.installSkills = async () => ({ status: 'error', error: { kind: 'custom', data: { message: 'UI fixture cannot install files' } } });

async function configure(options: Partial<Config>) {
  config = { ...initial, ...options };
  await getCurrentWindow().setFocus();
  await i18n.changeLanguage(config.language);
  applyPersistedAppearance(config.dark ? 'dark' : 'light');
  useSettingsStore.setState({ locale: config.language, theme: config.dark ? 'dark' : 'light' });
  useMutationStore.setState({ activeMutation: null });
  const originalHost = config.long ? 'engineering-guidelines.downloads.example.com' : 'docs.example.com';
  const actualHost = config.long ? 'content-distribution.engineering.example.net' : 'cdn.example.net';
  const source = `https://${originalHost}/skills.zip${config.oneTime ? '?token=temporary-fixture' : ''}`;
  const optionsList = Array.from({ length: 12 }, (_, index) => ({ id: `private-${index}`, kind: 'standardDirectory' as const, agentIds: [`agent-${index}`], displayName: index === 0 ? config.long ? 'Claude Code · Engineering quality assurance' : 'Claude Code' : `Team Agent ${index + 1}`, path: index === 0 && config.long ? `${projectPath}/.agent-resources/engineering/quality-assurance/skills` : `.team-${index + 1}/skills`, groupId: null, selectable: true, modeConstraint: index < 8 ? 'userSelectable' as const : 'copyOnly' as const, disabledReason: null }));
  selection = {
    agents: [...sharedNames.map((displayName, index) => ({ id: `shared-${index}`, displayName, kind: 'standard' as const, directoryAccess: 'standardOnly' as const, detection: 'detected' as const, installOptionId: null, groupId: null })), ...optionsList.map(option => ({ id: option.agentIds[0], displayName: option.displayName, kind: 'standard' as const, directoryAccess: 'privateOnly' as const, detection: 'detected' as const, installOptionId: option.id, groupId: null }))],
    installOptions: optionsList, groups: [], baselineSelectedOptionIds: optionsList.map(option => option.id), unavailableExplicitAgents: [], userModeOptionIds: optionsList.filter(option => option.modeConstraint === 'userSelectable').map(option => option.id), revision: 'fixture',
  };
  const names = Array.from({ length: config.count }, (_, index) => index === 0 ? config.long ? 'code-review-for-team-engineering-conventions-and-architecture-guidelines' : 'code-review' : `team-skill-${index + 1}`);
  wizardState = { step: config.step, entryPoint: 'skills-panel', scope: 'project', context, projectPath, environmentName: windows ? 'Windows' : 'Linux', sourceInput: source, source,
    fetchStatus: 'success', fetchError: null, gitRef: null, discoverySession: { sessionId: 'fixture', environment: native, sourceFingerprint: 'fixture', expiresAtEpochMs: Date.now() + 60_000 },
    redirectedDownloadHost: config.redirect ? actualHost : null, redirectAcknowledged: false,
    availableSkills: names.map(name => ({ name, installDirName: name, description: 'Team conventions', relativePath: `skills/${name}/SKILL.md`, pluginName: null })), selectedSkills: names, skillFilter: null, skillSearchQuery: '', overwrites: {}, preparation: { status: 'idle' }, agentSelectionIntent: { wildcardRequested: false, explicitAgentIds: [] }, installResults: null };
  fixture.wizardState = wizardState;
  fixture.lastPrepared = [];
  const at = (path: string) => ({ environment: native, nativePath: `${projectPath}/${path}` });
  useSkillUpdateWorkflow.setState({ phase: config.view === 'update' ? 'ready' : 'closed', context: config.view === 'update' ? context : null, skillNames: ['code-review'], selectedCopyEntries: new Set(),
    preview: { sources: [{ sourceKey: 'download', sourceDisplay: originalHost, refDisplay: '', skillNames: ['code-review'], error: null, downloadRedirect: config.redirect ? { originalHost, actualHost } : null }], blocked: [], redirectedDownloadHosts: config.redirect ? [actualHost] : [], skills: [{ skillName: 'code-review', sourceKey: 'download', sourceDisplay: originalHost, refDisplay: '', capability: { canRunUpdate: true, canCheckForUpdates: true, canCheckAutomatically: false, reason: null }, adapterTargets: [], cleanCopyCount: 0, blockingReasons: [], fallbackForecasts: [], targets: [{ displayPath: at('.agents/skills/code-review'), readers: [], restoring: false, isStandard: true, kind: 'directory' }], linkedTargets: [], overwritePrivateEntries: [] }] },
    prepare: async () => {}, confirm: async () => {}, cancel: async () => {}, acceptMutation: () => {},
  });
  const params = new URLSearchParams({ context: JSON.stringify(context), projectPath, environmentName: wizardState.environmentName! });
  root.render(<MemoryRouter key={++renderId} initialEntries={[`/wizard?${params}`]}><TooltipProvider><WindowLifecycleContext.Provider value={{ requestAction: async () => {} }}>
    {config.view === 'wizard' ? <WizardPage /> : <UpdatePlanDialog open context={context} skillNames={['code-review']} onOpenChange={() => {}} />}
  </WindowLifecycleContext.Provider></TooltipProvider></MemoryRouter>);
}
Object.assign(window, { configureDownloadMaintenance: configure });
await configure(initial);
