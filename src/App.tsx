import { useEffect } from 'react';
import { BrowserRouter, Routes, Route, Outlet } from 'react-router-dom';
import { listen } from '@tauri-apps/api/event';
import { Header } from '@/components/layout/Header';
import { SkillsPage } from '@/pages/SkillsPage';
import { DiscoverPage } from '@/pages/DiscoverPage';
import { SettingsPage } from '@/pages/SettingsPage';
import { WizardPage } from '@/pages/WizardPage';
import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useSkillsStore } from '@/stores/skills';

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

function App() {
  const fetchSkills = useSkillsStore((s) => s.fetchSkills);

  // 监听向导窗口完成事件
  useEffect(() => {
    const unlisten = listen('wizard-result', () => {
      fetchSkills();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [fetchSkills]);

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
      </TooltipProvider>
    </BrowserRouter>
  );
}

export default App;
