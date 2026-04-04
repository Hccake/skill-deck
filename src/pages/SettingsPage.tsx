import { useEffect, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Target, ExternalLink, FolderOpen, Trash2, Plus, Info, RefreshCw, Check, Github, Bug, Terminal, Briefcase } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { getVersion } from '@tauri-apps/api/app';
import { Card, CardContent } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';

import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { listAgents, getLastSelectedAgents, saveLastSelectedAgents } from '@/hooks/useTauriApi';
import { useContextStore } from '@/stores/context';
import { useUpdaterStore } from '@/stores/updater';
import { AgentSelector } from '@/components/skills/add-skill/AgentSelector';
import { Progress } from '@/components/ui/progress';
import { relaunchApp } from '@/stores/updater';
import type { AgentInfo } from '@/bindings';
import { COMPATIBLE_CLI_VERSION } from '@/constants';

import logoUrl from '@/assets/logo.png';

interface ProjectRowProps {
  path: string;
  onRemove?: (path: string) => void;
}

function ProjectRow({ path, onRemove }: ProjectRowProps) {
  const basename = path.split(/[/\\]/).pop() || path;
  
  return (
    <div className="flex items-center justify-between py-2.5 px-3 sm:px-4 group hover:bg-muted/30 transition-colors">
      <div className="flex items-center gap-3 sm:gap-3.5 min-w-0">
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-[10px] bg-muted/60 text-muted-foreground group-hover:bg-background group-hover:text-foreground transition-colors border border-border/40 shadow-sm">
          <FolderOpen className="h-4 w-4" />
        </div>
        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold text-foreground truncate">{basename}</span>
          <span className="text-[10px] font-mono text-muted-foreground truncate opacity-80 mt-0.5">{path}</span>
        </div>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8 text-muted-foreground/50 hover:text-destructive hover:bg-destructive/10 cursor-pointer flex-shrink-0 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-all"
        onClick={() => onRemove?.(path)}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  );
}

export function SettingsPage() {
  const { t } = useTranslation();

  // 状态管理
  const [allAgents, setAllAgents] = useState<AgentInfo[]>([]);
  const [selectedAgents, setSelectedAgents] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const { projects, projectsLoaded, loadProjects, addProject, removeProject } = useContextStore();
  const { status: updateStatus, newVersion, downloadProgress, lastCheckTime, checkForUpdate } = useUpdaterStore();

  const [version, setVersion] = useState('');

  // 动态获取应用版本号
  useEffect(() => {
    getVersion().then(setVersion);
  }, []);

  // 确保 projects 已加载
  useEffect(() => {
    if (!projectsLoaded) {
      loadProjects();
    }
  }, [projectsLoaded, loadProjects]);

  // 加载 agents 数据和默认选择
  useEffect(() => {
    async function fetchData() {
      try {
        setLoading(true);
        const [agentsData, lastSelected] = await Promise.all([
          listAgents(),
          getLastSelectedAgents(),
        ]);
        setAllAgents(agentsData);
        setSelectedAgents(lastSelected);
      } catch (e) {
        console.error('Failed to load data:', e);
      } finally {
        setLoading(false);
      }
    }
    fetchData();
  }, []);

  // 处理 agents 选择变化
  const handleSelectionChange = useCallback((agents: string[]) => {
    setSelectedAgents(agents);
    // 异步保存
    saveLastSelectedAgents(agents).catch((error) => {
      console.error('Failed to save agents:', error);
    });
  }, []);

  // Event handlers
  const handleAddProject = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t('settings.addProject'),
      });
      if (selected && typeof selected === 'string') {
        await addProject(selected);
      }
    } catch (error) {
      console.error('Failed to open folder picker:', error);
    }
  };

  // 检查是否有 Non-Universal agents
  const hasNonUniversalAgents = allAgents.some((a) => !a.isUniversal);

  return (
    <div className="flex flex-col h-full">
      {/* Content Area */}
      <div className="flex-1 overflow-auto px-4 sm:px-6 py-4 sm:py-5">
        {/* 居中容器 */}
        <div className="mx-auto max-w-xl lg:max-w-2xl">
          <Tabs defaultValue="general" className="w-full">
            <TabsList className="mb-5">
              <TabsTrigger value="general">{t('settings.tabs.general')}</TabsTrigger>
              <TabsTrigger value="projects">{t('settings.tabs.projects')}</TabsTrigger>
              <TabsTrigger value="about">{t('settings.tabs.about')}</TabsTrigger>
            </TabsList>

            {/* General Tab */}
            <TabsContent value="general" className="space-y-5 sm:space-y-6">
              <section>
                <div className="flex items-center gap-2 sm:gap-2.5 mb-3">
                  <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent">
                    <Target className="h-4 w-4 text-accent-foreground" />
                  </div>
                  <div>
                    <h2 className="text-sm font-heading font-bold text-foreground">
                      {t('settings.defaultAgents.title')}
                    </h2>
                    <p className="text-xs text-muted-foreground">
                      {t('settings.defaultAgents.description')}
                    </p>
                  </div>
                </div>

                {loading ? (
                  <div className="space-y-2 sm:space-y-3 animate-in fade-in duration-300">
                    {Array.from({ length: 3 }).map((_, i) => (
                      <div key={i} className="flex items-center gap-3 p-3 sm:p-4 rounded-xl border border-border/40 bg-accent/10">
                        <Skeleton className="h-10 w-10 rounded-lg shrink-0" />
                        <div className="flex-1 space-y-2">
                          <Skeleton className="h-4 w-1/3 max-w-[120px]" />
                          <Skeleton className="h-3 w-1/2 max-w-[200px]" />
                        </div>
                      </div>
                    ))}
                  </div>
                ) : !hasNonUniversalAgents ? (
                  <div className="relative overflow-hidden rounded-xl border border-dashed border-border/80 bg-accent/20 p-5 sm:p-6">
                    <div className="flex flex-col items-center text-center">
                      <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted mb-2.5">
                        <Target className="h-5 w-5 text-muted-foreground" />
                      </div>
                      <p className="text-sm font-medium text-foreground mb-1">
                        {t('settings.defaultAgents.empty')}
                      </p>
                      <p className="text-xs text-muted-foreground max-w-[220px]">
                        {t('settings.defaultAgents.emptyHint')}
                      </p>
                    </div>
                  </div>
                ) : (
                  <div className="space-y-3">
                    {/* 复用 AgentSelector 组件 */}
                    <AgentSelector
                      selectedAgents={selectedAgents}
                      allAgents={allAgents}
                      onSelectionChange={handleSelectionChange}
                    />

                    {/* CLI 共享提示 */}
                    <p className="text-xs text-muted-foreground flex items-center gap-1.5">
                      <Info className="h-3 w-3" />
                      {t('settings.defaultAgents.cliShared')}
                    </p>
                  </div>
                )}
              </section>
            </TabsContent>

            {/* Projects Tab */}
            <TabsContent value="projects" className="space-y-5 sm:space-y-6">
              <section>
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-2 sm:gap-2.5">
                    <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent">
                      <Briefcase className="h-4 w-4 text-accent-foreground" />
                    </div>
                    <div>
                      <h2 className="text-sm font-heading font-bold text-foreground">
                        {t('settings.projects')}
                      </h2>
                      <p className="text-xs text-muted-foreground">
                        {t('settings.projectsHint')}
                      </p>
                    </div>
                  </div>
                  <Button
                    size="sm"
                    className="gap-1.5 cursor-pointer shadow-sm font-medium h-8 bg-primary/10 text-primary hover:bg-primary/20 transition-all"
                    onClick={handleAddProject}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t('settings.addProject')}
                  </Button>
                </div>

                <Card className="py-0 gap-0 overflow-hidden shadow-sm border-border/60">
                  {projects.length === 0 ? (
                    <div className="relative bg-muted/10 p-8 sm:p-10 flex flex-col items-center text-center">
                      <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted mb-4 shadow-sm border border-border/50 ring-4 ring-muted/20">
                        <FolderOpen className="h-7 w-7 text-muted-foreground/70" />
                      </div>
                      <p className="text-[15px] font-semibold text-foreground mb-1.5">
                        {t('settings.projectsEmpty')}
                      </p>
                      <p className="text-xs text-muted-foreground max-w-[240px]">
                        {t('settings.projectsEmptyHint')}
                      </p>
                    </div>
                  ) : (
                    <CardContent className="p-0 divide-y divide-border/40">
                      {projects.map((path) => (
                        <ProjectRow
                          key={path}
                          path={path}
                          onRemove={(path) => removeProject(path)}
                        />
                      ))}
                    </CardContent>
                  )}
                </Card>
              </section>
            </TabsContent>

            {/* About Tab */}
            <TabsContent value="about" className="animate-in fade-in duration-500">
              <div className="flex flex-col items-center justify-center py-2 sm:py-4 max-w-xl mx-auto space-y-5 sm:space-y-6">
                
                {/* Hero Section */}
                <div className="flex flex-col items-center text-center space-y-2.5">
                  <div className="relative group">
                    <div className="absolute inset-x-4 inset-y-4 bg-primary/20 blur-2xl rounded-full opacity-60 group-hover:opacity-100 transition-opacity duration-700" />
                    <img 
                      src={logoUrl} 
                      alt="Logo" 
                      className="relative z-10 h-16 w-16 sm:h-20 sm:w-20 drop-shadow-xl transition-transform duration-500 hover:scale-[1.03]" 
                    />
                  </div>
                  <div>
                    <h2 className="text-xl sm:text-2xl font-heading font-extrabold text-foreground tracking-tight">
                      Skill Deck
                    </h2>
                    <div className="flex items-center gap-2 justify-center mt-1.5 text-sm text-muted-foreground">
                      <span className="font-mono bg-muted/60 px-2.5 py-0.5 rounded-md border border-border/50 text-[10px] font-medium text-foreground">
                        v{version || '...'}
                      </span>
                      <span className="opacity-40">·</span>
                      <span className="tracking-tight text-xs font-medium">{t('settings.aboutSections.tagline', 'Desktop Agent Manager')}</span>
                    </div>
                  </div>
                </div>

                {/* Bento Grid Links */}
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 w-full px-2 sm:px-0">
                  {/* GitHub */}
                  <a href="https://github.com/hccake/skill-deck" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-2xl border border-border/50 bg-card/40 hover:bg-card/80 shadow-sm hover:shadow-md transition-all group cursor-pointer h-24 sm:h-28">
                    <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-muted/80 group-hover:bg-background shadow-sm transition-colors mb-2 border border-border/50">
                      <Github className="h-4 w-4 text-foreground/80 group-hover:text-foreground transition-colors" />
                    </div>
                    <span className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{t('settings.links.openSource', '开源项目')}</span>
                    <span className="text-xs font-bold text-foreground flex items-center gap-1">
                      {t('settings.links.githubTitle', 'GitHub 仓库')} <ExternalLink className="h-2.5 w-2.5 opacity-40 ml-0.5" />
                    </span>
                  </a>

                  {/* Issues */}
                  <a href="https://github.com/hccake/skill-deck/issues" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-2xl border border-border/50 bg-card/40 hover:bg-card/80 shadow-sm hover:shadow-md transition-all group cursor-pointer h-24 sm:h-28">
                    <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-red-500/10 dark:bg-red-500/20 shadow-sm transition-colors mb-2 border border-red-500/10">
                      <Bug className="h-4 w-4 text-red-600 dark:text-red-400" />
                    </div>
                    <span className="text-[10px] font-semibold text-muted-foreground uppercase tracking-widest mb-1">{t('settings.links.feedback', '反馈意见')}</span>
                    <span className="text-xs font-bold text-foreground flex items-center gap-1">
                      {t('settings.links.issuesTitle', '提交 Issue')} <ExternalLink className="h-2.5 w-2.5 opacity-40 ml-0.5" />
                    </span>
                  </a>

                  {/* CLI Spec */}
                  <a href="https://github.com/vercel-labs/skills" target="_blank" rel="noopener noreferrer" className="flex flex-col items-center justify-center p-3.5 rounded-2xl border border-border/50 bg-card/40 hover:bg-card/80 shadow-sm hover:shadow-md transition-all group cursor-pointer h-24 sm:h-28">
                    <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-blue-500/10 dark:bg-blue-500/20 shadow-sm transition-colors mb-2 border border-blue-500/10">
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
                  {['Tauri v2', 'React 18', 'TypeScript', 'Tailwind'].map((tech) => (
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
                        <Button disabled className="h-9 px-6 rounded-full shadow-md gap-2 w-48 transition-all font-semibold">
                          <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                          {updateStatus === 'checking' ? t('settings.update.checking') : `${t('settings.update.downloading', '正在下载')} ${downloadProgress}%`}
                        </Button>
                        {updateStatus === 'downloading' && (
                          <Progress value={downloadProgress} className="h-1.5 w-48 opacity-80" />
                        )}
                      </div>
                    ) : updateStatus === 'ready' ? (
                      <div className="flex flex-col items-center space-y-2">
                        <Button onClick={() => relaunchApp()} className="h-9 px-6 rounded-full bg-emerald-600 hover:bg-emerald-700 text-white shadow-md hover:shadow-lg gap-2 w-48 transition-all font-semibold">
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
                          className="h-9 px-6 rounded-full bg-blue-600 hover:bg-blue-700 text-white shadow-md hover:shadow-lg gap-2 w-48 transition-all font-semibold hover:scale-105 active:scale-95"
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
                  
                  <div className="text-[10px] text-muted-foreground/50 text-center pb-2 font-medium">
                    Copyright &copy; {new Date().getFullYear()} hccake. All rights reserved.
                  </div>
                </div>

              </div>
            </TabsContent>
          </Tabs>
        </div>
      </div>
    </div>
  );
}
