import { useEffect, useState, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Target, ExternalLink, FolderOpen, X, Plus, Info, RefreshCw, Check } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { getVersion } from '@tauri-apps/api/app';
import { Card, CardContent } from '@/components/ui/card';
import { Separator } from '@/components/ui/separator';
import { Button } from '@/components/ui/button';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { listAgents, getLastSelectedAgents, saveLastSelectedAgents } from '@/hooks/useTauriApi';
import { useContextStore } from '@/stores/context';
import { useUpdaterStore } from '@/stores/updater';
import { AgentSelector } from '@/components/skills/add-skill/AgentSelector';
import type { AgentInfo } from '@/bindings';

interface ProjectRowProps {
  path: string;
  onRemove?: (path: string) => void;
}

function ProjectRow({ path, onRemove }: ProjectRowProps) {
  return (
    <div className="flex items-center justify-between py-3 px-3 sm:px-4">
      <div className="flex items-center gap-2 sm:gap-3 min-w-0">
        <FolderOpen className="h-4 w-4 text-muted-foreground flex-shrink-0" />
        <span className="text-sm font-mono text-foreground truncate">
          {path}
        </span>
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="h-7 w-7 text-muted-foreground hover:text-destructive hover:bg-destructive/10 cursor-pointer flex-shrink-0"
        onClick={() => onRemove?.(path)}
      >
        <X className="h-3.5 w-3.5" />
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
  const { status: updateStatus, checkForUpdate } = useUpdaterStore();

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
                    <h2 className="text-sm font-semibold text-foreground">
                      {t('settings.defaultAgents.title')}
                    </h2>
                    <p className="text-xs text-muted-foreground">
                      {t('settings.defaultAgents.description')}
                    </p>
                  </div>
                </div>

                {loading ? (
                  <div className="text-center py-4 text-muted-foreground text-sm">
                    {t('common.loading')}
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
                <div className="flex items-center gap-2 sm:gap-2.5 mb-3">
                  <div className="flex h-7 w-7 items-center justify-center rounded-lg bg-accent">
                    <FolderOpen className="h-4 w-4 text-accent-foreground" />
                  </div>
                  <div>
                    <h2 className="text-sm font-semibold text-foreground">
                      {t('settings.projects')}
                    </h2>
                    <p className="text-xs text-muted-foreground">
                      {t('settings.projectsHint')}
                    </p>
                  </div>
                </div>

                {projects.length === 0 ? (
                  <div className="relative overflow-hidden rounded-xl border border-dashed border-border/80 bg-accent/20 p-5 sm:p-6">
                    <div className="flex flex-col items-center text-center">
                      <div className="flex h-10 w-10 items-center justify-center rounded-full bg-muted mb-2.5">
                        <FolderOpen className="h-5 w-5 text-muted-foreground" />
                      </div>
                      <p className="text-sm font-medium text-foreground mb-1">
                        {t('settings.projectsEmpty')}
                      </p>
                      <p className="text-xs text-muted-foreground max-w-[220px]">
                        {t('settings.projectsEmptyHint')}
                      </p>
                    </div>
                  </div>
                ) : (
                  <Card className="py-0 gap-0">
                    <CardContent className="p-0 divide-y divide-border/40">
                      {projects.map((path) => (
                        <ProjectRow
                          key={path}
                          path={path}
                          onRemove={(path) => removeProject(path)}
                        />
                      ))}
                    </CardContent>
                  </Card>
                )}

                <div className="mt-3 flex justify-end">
                  <Button
                    variant="outline"
                    size="sm"
                    className="gap-1.5 cursor-pointer"
                    onClick={handleAddProject}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t('settings.addProject')}
                  </Button>
                </div>
              </section>
            </TabsContent>

            {/* About Tab */}
            <TabsContent value="about" className="space-y-5 sm:space-y-6">
              <section>
                <h2 className="text-sm font-semibold text-foreground mb-3">
                  {t('settings.about')}
                </h2>
                <Card className="py-0 gap-0">
                  <CardContent className="p-0">
                    <div className="flex items-center justify-between py-3 px-3 sm:px-4">
                      <span className="text-sm text-muted-foreground">{t('settings.version')}</span>
                      <span className="text-sm font-medium text-foreground font-mono">
                        v{version}
                      </span>
                    </div>
                    <Separator />
                    <div className="flex items-center justify-between py-3 px-3 sm:px-4">
                      <span className="text-sm text-muted-foreground">
                        {t('settings.compatible')}
                      </span>
                      <a
                        href="https://github.com/vercel-labs/skills"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-1.5 text-sm text-primary hover:text-primary/80 transition-hover cursor-pointer"
                      >
                        <span>vercel-labs/skills</span>
                        <ExternalLink className="h-3.5 w-3.5" />
                        <span className="sr-only">({t('skills.externalLink')})</span>
                      </a>
                    </div>
                    <Separator />
                    <div className="flex items-center justify-between py-3 px-3 sm:px-4">
                      <span className="text-sm text-muted-foreground">
                        {t('settings.update.checkForUpdates')}
                      </span>
                      {updateStatus === 'checking' ? (
                        <Button variant="ghost" size="sm" disabled className="gap-1.5">
                          <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                          <span className="text-xs">{t('settings.update.checking')}</span>
                        </Button>
                      ) : updateStatus === 'idle' && localStorage.getItem('updater_last_check') ? (
                        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                          <Check className="h-3.5 w-3.5 text-green-500" />
                          <span>{t('settings.update.upToDate')}</span>
                        </div>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1.5 cursor-pointer"
                          onClick={() => checkForUpdate()}
                        >
                          <RefreshCw className="h-3.5 w-3.5" />
                          <span className="text-xs">{t('settings.update.checkForUpdates')}</span>
                        </Button>
                      )}
                    </div>
                  </CardContent>
                </Card>
              </section>
            </TabsContent>
          </Tabs>
        </div>
      </div>
    </div>
  );
}
