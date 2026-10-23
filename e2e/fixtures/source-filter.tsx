// 原生验收使用实际工具栏、来源筛选、卡片和引用组件；数据与操作仅限此夹具。
import { useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/manrope/600.css';
import '@/index.css';
import i18n from '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SkillsToolbar } from '@/components/skills/SkillsToolbar';
import { GitReference, GitReferenceField } from '@/components/skills/GitReference';
import { SkillCard } from '@/components/skills/SkillCard';
import { filterSkills, getSourceFilterOptions } from '@/lib/skills/filter';
import { useSkillSourceFilter } from '@/hooks/useSkillSourceFilter';
import type { InstalledSkill, ResolvedAgent } from '@/bindings';

const sha = 'a1b2c3d4e5f678901234567890abcdef1234567890';
const scope = { enabled: true, readsStandard: true, standardPath: '~/.agents/skills', privatePath: null, readPaths: ['~/.agents/skills'], standardPresence: 'present' as const, privatePresence: null, legacyPaths: [] };
const agent: ResolvedAgent = {
  definition: { id: 'codex', displayName: 'Codex', source: 'builtin', aliases: [],
    global: { enabled: true, readsStandard: true, privatePath: null }, project: { enabled: true, readsStandard: true, privatePath: null },
    detection: { kind: 'anyPathExists', paths: [] }, legacyPaths: [], adapter: 'standard' },
  detection: 'detected', detectionReason: null, global: scope, project: scope,
};
const skills: InstalledSkill[] = ['review', 'release', 'notes'].map((name, index) => ({
  name, description: 'Source filter and pinned reference layout', path: `C:\\Skills\\${name}`, canonicalPath: `C:\\Skills\\${name}`,
  scope: 'global', agents: ['codex'], associatedAgents: ['codex'], source: index === 2 ? 'other/tools' : 'acme/tools',
  sourceUrl: `https://github.com/${index === 2 ? 'other' : 'acme'}/tools`, gitRef: index === 0 ? sha : 'main', refKind: index === 0 ? 'commit' : 'branch',
  sourceFilter: { key: index === 2 ? 'other' : 'acme', kind: 'git', label: index === 2 ? 'other/tools' : 'acme/tools', detail: 'github.com' },
}));
type Config = { width: number; language: 'zh-CN' | 'en'; dark: boolean; long: boolean; sourceLabel?: string };
const initial: Config = { width: 240, language: 'zh-CN', dark: false, long: false };

function Fixture() {
  const [config, setConfig] = useState(initial);
  const [query, setQuery] = useState('');
  const [selectedAgent, setSelectedAgent] = useState<string | null>(null);
  const options = useMemo(() => getSourceFilterOptions(skills.map(skill => config.sourceLabel && skill.sourceFilter?.key === 'acme'
    ? { ...skill, sourceFilter: { ...skill.sourceFilter, label: config.sourceLabel } } : skill)), [config.sourceLabel]);
  const sourceFilter = useSkillSourceFilter('native/global', options, false);
  const visible = filterSkills(skills, query, selectedAgent, sourceFilter.selected?.key ?? null);
  useEffect(() => {
    Object.assign(window, { configureSourceFilter: async (next: Config) => {
      await i18n.changeLanguage(next.language);
      document.documentElement.classList.toggle('dark', next.dark);
      setConfig(next); setQuery(''); setSelectedAgent(null); sourceFilter.select(null);
    } });
    return () => { delete (window as { configureSourceFilter?: unknown }).configureSourceFilter; };
  }, [sourceFilter]);
  return <TooltipProvider><main className="flex min-h-screen bg-background text-foreground">
    <section data-testid="source-list" className="shrink-0 overflow-hidden border-r p-3" style={{ width: config.width }}>
      <div data-testid="source-toolbar"><SkillsToolbar compact={config.width <= 320} searchQuery={query} onSearchChange={setQuery}
        selectedAgent={selectedAgent} onAgentChange={setSelectedAgent} filterableAgents={[agent]} agentMatchCounts={new Map([['codex', 3]])}
        totalSkillCount={3} onSync={() => {}} sourceOptions={sourceFilter.options} selectedSource={sourceFilter.selected} onSourceChange={sourceFilter.select} /></div>
      <div data-testid="visible-skills" className="mt-4 space-y-3">{visible.map(skill => config.width > 320
        ? <SkillCard key={skill.name} skill={skill} displayScope="global" />
        : <div key={skill.name} className="min-w-0"><p className="text-sm">{skill.name}</p><p className="truncate text-xs text-muted-foreground">{skill.source}</p></div>)}</div>
      <div data-testid="reference-preview" className="mt-5 min-w-0"><GitReference value={config.long ? 'release/very-long-reference-for-native-layout-verification-and-truncation' : sha} kind={config.long ? 'branch' : 'commit'} /></div>
    </section>
    {config.width <= 320 ? <aside className="min-w-0 flex-1 p-5"><h2 className="mb-5 text-base font-semibold">review</h2><GitReferenceField value={sha} kind="commit" /></aside> : null}
  </main></TooltipProvider>;
}

void i18n.changeLanguage('zh-CN').then(() => createRoot(document.getElementById('root')!).render(<Fixture />));
