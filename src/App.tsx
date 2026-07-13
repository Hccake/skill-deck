import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Outlet } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Header } from '@/components/layout/Header';
import { SkillsPage } from '@/pages/SkillsPage';
import { DiscoverPage } from '@/pages/DiscoverPage';
import { SettingsPage } from '@/pages/SettingsPage';
import { WizardPage } from '@/pages/WizardPage';
import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useSkillsDataStore } from '@/stores/skills-data';
import { useEnvironmentStore } from '@/stores/environment';
import { useUpdaterStore } from '@/stores/updater';
import { UpdateDialog } from '@/components/update-dialog';

/** 主窗口布局 — 带 Header + Toaster */
function MainLayout() {
  return (
    <div className="flex flex-col h-screen bg-background text-foreground overflow-hidden">
      <Header />
      <main className="flex-1 flex flex-col overflow-hidden">
        <Outlet />
      </main>
      <Toaster />
    </div>
  );
}

// advanced-init-once: 防止 Strict Mode 双调用
let didInit = false;
let didDiscoverEnvironments = false;

function App() {
  const { t } = useTranslation();
  const fetchSkills = useSkillsDataStore((s) => s.fetchSkills);
  const discoverEnvironments = useEnvironmentStore((s) => s.discoverEnvironments);
  // rerender-defer-reads: 不订阅 error，减少不必要的 App 重渲染
  const { status, checkForUpdate, shouldAutoCheck } = useUpdaterStore();

  // 监听向导窗口完成事件
  useEffect(() => {
    const unlisten = listen('wizard-result', () => {
      fetchSkills();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchSkills]);

  // advanced-init-once: 启动时自动检查更新，guard 防止 Strict Mode 双调用
  useEffect(() => {
    if (didInit) return;
    didInit = true;
    if (shouldAutoCheck()) {
      checkForUpdate();
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (window.location.pathname === '/wizard' || didDiscoverEnvironments) return;
    didDiscoverEnvironments = true;
    void discoverEnvironments().catch((error) => {
      console.error('Failed to discover environments:', error);
    });
  }, [discoverEnvironments]);

  // 错误时弹 toast — rerender-defer-reads: 用 getState() 按需读取 error
  useEffect(() => {
    if (status === 'error') {
      const error = useUpdaterStore.getState().error;
      if (error) toast.error(t('settings.update.checkError'));
    }
  }, [status, t]);

  const showUpdateDialog = status === 'available' || status === 'downloading' || status === 'ready';

  return (
    <BrowserRouter>
      <TooltipProvider>
        <Routes>
          {/* 向导窗口路由 — 独立布局，无 Header，必须在通配符之前 */}
          <Route path="/wizard" element={<WizardPage />} />

          {/* 主窗口路由 — Layout Route 包裹 */}
          <Route element={<MainLayout />}>
            <Route path="/" element={<SkillsPage />} />
            <Route path="/discover" element={<DiscoverPage />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Route>
        </Routes>
        <UpdateDialog open={showUpdateDialog} />
      </TooltipProvider>
    </BrowserRouter>
  );
}

export default App;
