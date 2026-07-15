import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink, RefreshCw, Check, Github, Bug, Terminal } from 'lucide-react';
import { getVersion } from '@tauri-apps/api/app';
import { Button } from '@/components/ui/button';
import { useUpdaterStore } from '@/stores/updater';
import { Progress } from '@/components/ui/progress';
import { COMPATIBLE_CLI_VERSION } from '@/constants';
import { useWindowLifecycle } from '@/lifecycle/useWindowLifecycle';

import logoUrl from '@/assets/logo.png';

export function AboutTab() {
  const { t } = useTranslation();
  const { status: updateStatus, newVersion, downloadProgress, lastCheckTime, checkForUpdate } = useUpdaterStore();
  const { requestAction } = useWindowLifecycle();

  const [version, setVersion] = useState('');

  // 动态获取应用版本号
  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  return (
    <div className="flex flex-col h-full relative py-3 sm:py-5 max-w-xl mx-auto">
      <div className="flex-1 flex flex-col items-center justify-center space-y-5 w-full">

        {/* Hero Section */}
      <div className="flex flex-col items-center text-center space-y-2.5">
        <div className="relative group">
          <div className="absolute inset-x-5 inset-y-5 bg-primary/15 blur-2xl rounded-full opacity-70 transition-all duration-500 group-hover:opacity-100 group-hover:saturate-150 group-hover:blur-3xl" />
          <img
            src={logoUrl}
            alt="Logo"
            className="relative z-10 h-16 w-16 sm:h-[72px] sm:w-[72px] drop-shadow-md transition-all duration-500 group-hover:scale-[1.05] group-hover:drop-shadow-lg"
          />
        </div>
        <div>
          <h2 className="text-xl sm:text-2xl font-heading font-bold text-foreground tracking-tight">
            Skill Deck
          </h2>
          <div className="flex items-center gap-2 justify-center mt-1.5 text-sm text-muted-foreground">
            <span className="font-mono bg-muted/60 px-2.5 py-0.5 rounded-md border border-border/50 text-[10px] font-medium text-foreground">
              v{version || '...'}
            </span>
            <span className="opacity-40">&middot;</span>
            <span className="tracking-tight text-xs font-medium">{t('settings.aboutSections.tagline', 'Desktop Agent Manager')}</span>
          </div>
        </div>
      </div>

      {/* Bento Grid Links */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 w-full px-2 sm:px-0">
        {/* GitHub */}
        <a href="https://github.com/hccake/skill-deck" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-lg border border-border/60 bg-background/70 hover:bg-muted/30 shadow-xs transition-colors group cursor-pointer h-24 sm:h-[104px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted/60 group-hover:bg-background transition-colors mb-2 border border-border/50">
            <Github className="h-4 w-4 text-foreground/80 group-hover:text-foreground transition-colors" />
          </div>
          <span className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{t('settings.links.openSource', '开源项目')}</span>
          <span className="text-xs font-bold text-foreground flex items-center gap-1">
            {t('settings.links.githubTitle', 'GitHub 仓库')} <ExternalLink className="h-2.5 w-2.5 opacity-40 ml-0.5" />
          </span>
        </a>

        {/* Issues */}
        <a href="https://github.com/hccake/skill-deck/issues" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-lg border border-border/60 bg-background/70 hover:bg-muted/30 shadow-xs transition-colors group cursor-pointer h-24 sm:h-[104px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-red-500/10 dark:bg-red-500/15 transition-colors mb-2 border border-red-500/10">
            <Bug className="h-4 w-4 text-red-600 dark:text-red-400" />
          </div>
          <span className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{t('settings.links.feedback', '反馈意见')}</span>
          <span className="text-xs font-bold text-foreground flex items-center gap-1">
            {t('settings.links.issuesTitle', '提交 Issue')} <ExternalLink className="h-2.5 w-2.5 opacity-40 ml-0.5" />
          </span>
        </a>

        {/* CLI Spec */}
        <a href="https://github.com/vercel-labs/skills" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-lg border border-border/60 bg-background/70 hover:bg-muted/30 shadow-xs transition-colors group cursor-pointer h-24 sm:h-[104px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40">
          <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-blue-500/10 dark:bg-blue-500/15 transition-colors mb-2 border border-blue-500/10">
            <Terminal className="h-4 w-4 text-blue-600 dark:text-blue-400" />
          </div>
          <span className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{t('settings.links.cliCompatibility', 'CLI 兼容')} v{COMPATIBLE_CLI_VERSION}</span>
          <span className="text-xs font-bold text-foreground flex items-center gap-1">
            {t('settings.links.vercelSkills', 'Vercel Skills')} <ExternalLink className="h-2.5 w-2.5 opacity-40 ml-0.5" />
          </span>
        </a>
      </div>

      {/* Tech Stack Badges */}
      <div className="flex flex-wrap items-center justify-center gap-2 pt-1">
        {['Tauri v2', 'React 19', 'TypeScript', 'Tailwind'].map((tech) => (
          <span key={tech} className="px-2.5 py-0.5 text-[10px] font-medium text-muted-foreground bg-muted/40 hover:bg-muted/60 transition-colors rounded-full border border-border/40">
            {tech}
          </span>
        ))}
      </div>

      {/* Primary Action (Update) & Copyright */}
      <div className="flex flex-col items-center space-y-4 w-full">
        <div className="flex flex-col items-center justify-center min-h-[60px]">
          {updateStatus === 'checking' || updateStatus === 'downloading' ? (
            <div className="flex flex-col items-center space-y-2">
              <Button disabled className="h-9 px-5 rounded-lg shadow-xs gap-2 w-48 transition-all font-semibold">
                <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                {updateStatus === 'checking' ? t('settings.update.checking') : `${t('settings.update.downloading', '正在下载')} ${downloadProgress}%`}
              </Button>
              {updateStatus === 'downloading' && (
                <Progress value={downloadProgress} className="h-1.5 w-48 opacity-80" />
              )}
            </div>
          ) : updateStatus === 'ready' ? (
            <div className="flex flex-col items-center space-y-2">
              <Button onClick={() => void requestAction('restartApplication')} className="h-9 px-5 rounded-lg bg-emerald-600 hover:bg-emerald-700 text-white shadow-xs gap-2 w-48 transition-all font-semibold">
                <RefreshCw className="h-3.5 w-3.5" />
                {t('settings.update.restartNow')}
              </Button>
              <span className="text-[11px] text-emerald-600 font-medium tracking-tight">
                {t('settings.update.readyToRestart', { version: newVersion })}
              </span>
            </div>
          ) : (
            <div className="flex flex-col items-center space-y-2">
              <Button
                onClick={() => checkForUpdate()}
                className="h-9 px-5 rounded-lg bg-primary text-primary-foreground hover:bg-primary/90 shadow-xs gap-2 w-48 transition-all font-semibold active:scale-[0.98]"
              >
                <RefreshCw className="h-3.5 w-3.5" />
                {t('settings.update.checkForUpdates', '检测更新')}
              </Button>
              <span className="text-[11px] text-muted-foreground flex items-center gap-1 font-medium">
                {updateStatus === 'available' ? (
                  <span className="text-primary">{t('settings.update.updateAvailable', { version: newVersion })}</span>
                ) : updateStatus === 'error' ? (
                  <span className="text-destructive">{t('settings.update.checkError')}</span>
                ) : lastCheckTime ? (
                  <>
                    <Check className="h-3 w-3 text-emerald-500" />
                    {t('settings.update.upToDate', '已是最新版')}
                  </>
                ) : (
                  t('settings.update.neverChecked', '检查以获取最新版本')
                )}
              </span>
            </div>
          )}
        </div>

      </div>
      </div>

      <div className="mt-auto text-[10px] text-muted-foreground/50 text-center pt-8 pb-2 font-medium">
        Copyright &copy; {new Date().getFullYear()} hccake. All rights reserved.
      </div>

    </div>
  );
}
