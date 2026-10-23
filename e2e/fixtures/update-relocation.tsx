// 原生布局夹具使用实际预览组件；确认操作不执行文件写入。
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/manrope/600.css';
import '@/index.css';
import i18n from '@/i18n';
import { UpdatePlanDialog } from '@/components/skills/UpdatePlanDialog';
import { SourcePathChange } from '@/components/skills/SourcePathChange';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useSkillUpdateWorkflow } from '@/workflows/skill-update';
import type { SkillLocationRef, SourcePathChange as PathChange, UpdateSkillPreview } from '@/bindings';

const context: SkillLocationRef = { environment: { kind: 'native' }, scope: { scope: 'global' } };
type Config = { language: 'zh-CN' | 'en'; long: boolean; library: boolean; dark: boolean };
const initial: Config = { language: 'zh-CN', long: false, library: false, dark: false };
const shortChange = { previousPath: 'skills/review', nextPath: 'tools/review' };
const longChange = { previousPath: 'skills/platform-engineering/team-development/cross-platform-architecture-review', nextPath: 'plugins/engineering-quality/skills/cross-platform-architecture-review' };

function Fixture() {
  const [config, setConfig] = useState(initial);
  const name = config.long ? 'cross-platform-architecture-review' : 'review';
  const change: PathChange = config.long ? longChange : shortChange;
  useEffect(() => {
    const configure = async (next: Config) => {
      await i18n.changeLanguage(next.language);
      document.documentElement.classList.toggle('dark', next.dark);
      const skillName = next.long ? 'cross-platform-architecture-review' : 'review';
      const preview: UpdateSkillPreview = {
        skillName, pathChange: next.long ? longChange : shortChange,
        sourceKey: 'source-1', sourceDisplay: 'acme/engineering-skills', refDisplay: 'main',
        capability: { canRunUpdate: true, canCheckForUpdates: true, reason: null },
        adapterTargets: [], cleanCopyCount: 0, blockingReasons: [], fallbackForecasts: [], linkedTargets: [],
        targets: [{ displayPath: { environment: context.environment, nativePath: `C:\\Skills\\${skillName}` }, readers: [], restoring: false, isStandard: true, kind: 'directory' }],
        overwritePrivateEntries: [{ entryId: 'cursor-copy', readers: [{ agentId: 'cursor', displayName: 'Cursor', logicalTargetId: 'cursor' }], displayPath: { environment: context.environment, nativePath: `C:\\Users\\User\\.cursor\\skills\\${skillName}` } }],
      };
      useSkillUpdateWorkflow.setState({ phase: 'ready', context, skillNames: [skillName], preview: { sources: [], blocked: [], redirectedDownloadHosts: [], skills: [preview] }, selectedCopyEntries: new Set(), confirm: async () => {} });
      setConfig(next);
    };
    Object.assign(window, { configureUpdateRelocation: configure });
    void configure(initial);
    return () => { delete (window as { configureUpdateRelocation?: unknown }).configureUpdateRelocation; };
  }, []);
  return <TooltipProvider>{config.library ? (
    <Dialog open><DialogContent>
      <DialogHeader><DialogTitle>{i18n.t('libraries.confirmUpdateTitle')}</DialogTitle></DialogHeader>
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1" data-testid="library-path-preview">
        <h3 className="max-w-full shrink-0 break-words text-sm font-semibold">{name}</h3>
        <SourcePathChange change={change} />
      </div>
      <DialogFooter><Button variant="outline">{i18n.t('common.cancel')}</Button><Button>{i18n.t('libraries.update')}</Button></DialogFooter>
    </DialogContent></Dialog>
  ) : <UpdatePlanDialog open context={context} skillNames={[name]} onOpenChange={() => {}} />}</TooltipProvider>;
}

void i18n.changeLanguage('zh-CN').then(() => createRoot(document.getElementById('root')!).render(<Fixture />));
