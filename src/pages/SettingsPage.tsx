import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { useContextStore } from '@/stores/context';
import { GeneralTab } from '@/components/settings/GeneralTab';
import { ProjectsTab } from '@/components/settings/ProjectsTab';
import { AboutTab } from '@/components/settings/AboutTab';

export function SettingsPage() {
  const { t } = useTranslation();
  const { projectsLoaded, loadProjects } = useContextStore();

  // 确保 projects 已加载
  useEffect(() => {
    if (!projectsLoaded) {
      loadProjects();
    }
  }, [projectsLoaded, loadProjects]);

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

            <TabsContent value="general">
              <GeneralTab />
            </TabsContent>

            <TabsContent value="projects">
              <ProjectsTab />
            </TabsContent>

            <TabsContent value="about" className="animate-in fade-in duration-500">
              <AboutTab />
            </TabsContent>
          </Tabs>
        </div>
      </div>
    </div>
  );
}
