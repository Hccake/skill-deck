import { lazy, Suspense, useEffect } from 'react';
import {
  createBrowserRouter,
  Route,
  RouterProvider,
  Routes,
  useLocation,
} from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Toaster } from '@/components/ui/sonner';
import { TooltipProvider } from '@/components/ui/tooltip';
import { useUpdaterStore } from '@/stores/updater';
import { UpdateDialog } from '@/components/update-dialog';
import { WindowLifecycleProvider } from '@/lifecycle/WindowLifecycleProvider';
import { UnsavedChangesProvider } from '@/lifecycle/UnsavedChangesProvider';

const MainLayout = lazy(() => import('@/layouts/MainLayout'));
const SkillsPage = lazy(() => import('@/pages/SkillsPage').then((module) => ({ default: module.SkillsPage })));
const LibraryPage = lazy(() => import('@/pages/LibraryPage').then((module) => ({ default: module.LibraryPage })));
const DiscoverPage = lazy(() => import('@/pages/DiscoverPage').then((module) => ({ default: module.DiscoverPage })));
const SettingsPage = lazy(() => import('@/pages/SettingsPage').then((module) => ({ default: module.SettingsPage })));
const WizardPage = lazy(() => import('@/pages/WizardPage').then((module) => ({ default: module.WizardPage })));

// advanced-init-once: 防止 Strict Mode 双调用
let didInit = false;

function RouteFallback() {
  const { t } = useTranslation();
  return (
    <div role="status" aria-live="polite" className="flex h-full items-center justify-center text-sm text-muted-foreground">
      {t('common.loading')}
    </div>
  );
}

function ApplicationShell() {
  const { t } = useTranslation();
  const { pathname } = useLocation();
  const isInstallWizard = pathname === '/wizard';
  const status = useUpdaterStore((state) => state.status);
  const dialogVisible = useUpdaterStore((state) => state.dialogVisible);
  const checkForUpdate = useUpdaterStore((state) => state.checkForUpdate);
  const shouldAutoCheck = useUpdaterStore((state) => state.shouldAutoCheck);

  // advanced-init-once: 启动时自动检查更新，guard 防止 Strict Mode 双调用
  useEffect(() => {
    if (didInit || isInstallWizard) return;
    didInit = true;
    if (shouldAutoCheck()) {
      checkForUpdate();
    }
  }, [isInstallWizard]); // eslint-disable-line react-hooks/exhaustive-deps

  // 错误时弹 toast — rerender-defer-reads: 用 getState() 按需读取 error
  useEffect(() => {
    if (status === 'error') {
      const updater = useUpdaterStore.getState();
      if (updater.error) {
        toast.error(t(
          updater.failedOperation === 'install'
            ? 'settings.update.installError'
            : 'settings.update.checkError',
        ));
      }
    }
  }, [status, t]);

  const showUpdateDialog = !isInstallWizard && dialogVisible
    && ['available', 'downloading', 'cancelling', 'installing', 'ready', 'error'].includes(status);

  return (
    <UnsavedChangesProvider>
      <WindowLifecycleProvider>
        <TooltipProvider>
          <Routes>
            {/* 向导窗口路由 — 独立布局，无 Header，必须在通配符之前 */}
            <Route path="/wizard" element={(
              <Suspense fallback={<RouteFallback />}>
                <WizardPage />
              </Suspense>
            )} />

            {/* 主窗口路由 — Layout Route 包裹 */}
            <Route element={(
              <Suspense fallback={<RouteFallback />}>
                <MainLayout />
              </Suspense>
            )}>
              <Route path="/" element={<SkillsPage />} />
              <Route path="/libraries" element={<LibraryPage />} />
              <Route path="/discover" element={<DiscoverPage />} />
              <Route path="/settings" element={<SettingsPage />} />
            </Route>
          </Routes>
          <UpdateDialog open={showUpdateDialog} />
          <Toaster />
        </TooltipProvider>
      </WindowLifecycleProvider>
    </UnsavedChangesProvider>
  );
}

const appRouter = createBrowserRouter([{
  path: '*',
  element: <ApplicationShell />,
}]);

function App() {
  return <RouterProvider router={appRouter} />;
}

export default App;
