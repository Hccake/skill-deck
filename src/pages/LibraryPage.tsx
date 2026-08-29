import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useGroupRef } from 'react-resizable-panels';
import {
  BookOpen,
  Check,
  ArrowUpCircle,
  CircleAlert,
  Library,
  Plus,
  RefreshCw,
  Search,
  X,
} from 'lucide-react';
import { useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { ScrollArea } from '@/components/ui/scroll-area';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import {
  LibraryCompactItem,
  LibraryAddDialog,
  DeleteLibraryDialog,
  LibrarySidebar,
  LibrarySkillCard,
  LibrarySkillDetailPanel,
  LibraryUsageLine,
} from '@/components/library';
import { useLibraryWorkspace } from '@/hooks/useLibraryWorkspace';
import { useBusinessWriteBlocked } from '@/hooks/useBusinessWriteBlocked';
import { useWorkspaceContextStore } from '@/stores/workspace-context';
import { useEnvironmentStore } from '@/stores/environment';
import { environmentKey } from '@/lib/context';
import { environmentDisplayName } from '@/lib/environments/presentation';
import { cn } from '@/lib/utils';
import { formatAppError } from '@/utils/format-app-error';
import { removeLibrarySkill, readLibrarySkillContent } from '@/hooks/useTauriApi';
import { useLibraryUpdateWorkflow } from '@/workflows/library-update';
import { libraryUpdateDisplayStatuses } from '@/lib/libraries/update-progress';
import {
  formatLibraryUpdateSummaryItems,
  summarizeLibraryUpdates,
} from '@/lib/libraries/update-summary';
import type {
  AppError,
  EnvironmentRef,
  LibraryId,
  SkillLibrarySummary,
} from '@/bindings';
import type {
  ExecuteLibraryCommand,
  LibraryAddTarget,
} from '@/components/library/useLibraryAddFlow';
import {
  captureLibraryDeletion,
  type LibraryDeletionRequest,
} from '@/workflows/library-deletion';

// react-resizable-panels v4 把裸数字当作像素，百分比必须写成字符串。
const SPLIT_VIEW_LAYOUT = {
  'library-skills-list-panel': 22,
  'library-skill-detail-panel': 78,
} as const;

const LIST_VIEW_LAYOUT = {
  'library-skills-list-panel': 100,
} as const;

export function LibraryPage() {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedLibraryId = searchParams.get('library');
  const environment = useWorkspaceContextStore((state) => state.selectedContext.environment);
  const environments = useEnvironmentStore((state) => state.environments);
  const workspace = useLibraryWorkspace(environment);

  // 选择先提交到 Workspace，下面的路由协调 effect 再同步实际选中项。
  const selectLibrary = useCallback((libraryId: LibraryId) => {
    return workspace.execute({ kind: 'select', libraryId });
  }, [workspace]);
  const writeBlocked = useBusinessWriteBlocked();
  const checks = useLibraryUpdateWorkflow((state) => state.checks);
  const updatePhase = useLibraryUpdateWorkflow((state) => state.phase);
  const updateError = useLibraryUpdateWorkflow((state) => state.hasError);
  const pendingUpdate = useLibraryUpdateWorkflow((state) => state.pending);
  const lastResults = useLibraryUpdateWorkflow((state) => state.lastResults);
  const activateUpdate = useLibraryUpdateWorkflow((state) => state.activate);
  const checkUpdates = useLibraryUpdateWorkflow((state) => state.check);
  const prepareUpdates = useLibraryUpdateWorkflow((state) => state.prepare);
  const confirmUpdates = useLibraryUpdateWorkflow((state) => state.confirm);
  const cancelUpdates = useLibraryUpdateWorkflow((state) => state.cancel);
  const resetUpdates = useLibraryUpdateWorkflow((state) => state.reset);

  const [nameDialog, setNameDialog] = useState<{ mode: 'create' | 'rename'; id?: LibraryId } | null>(null);
  const [name, setName] = useState('');
  const [nameError, setNameError] = useState<AppError | null>(null);
  const [addRequest, setAddRequest] = useState<{
    target: LibraryAddTarget;
    existingSkillNames: ReadonlySet<string>;
    execute: ExecuteLibraryCommand;
  } | null>(null);
  const [maintenanceBusy, setMaintenanceBusy] = useState(false);
  // 读取来源、移除成员和删除库的失败原因不同，不能共用一句通用文案。
  const [pageError, setPageError] = useState<{ scope: 'remove'; error: AppError } | null>(null);
  const [removeSkillName, setRemoveSkillName] = useState<string | null>(null);
  const [deleteRequest, setDeleteRequest] = useState<LibraryDeletionRequest | null>(null);
  const [query, setQuery] = useState('');
  const [selectedSkillName, setSelectedSkillName] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<string | null>(null);
  const [contentError, setContentError] = useState(false);

  const layoutRef = useGroupRef();
  const addTriggerRef = useRef<HTMLButtonElement | null>(null);
  const previousSplitViewRef = useRef(false);
  const committedRouteKeyRef = useRef<string | null>(null);
  const maintenanceRequestId = useRef(0);
  const contentRequestId = useRef(0);
  const activeLibraryTarget = useRef({
    environmentKey: environmentKey(environment),
    libraryId: workspace.selectedLibraryId,
  });
  activeLibraryTarget.current = {
    environmentKey: environmentKey(environment),
    libraryId: workspace.selectedLibraryId,
  };
  const routeSyncKey = `${environmentKey(environment)}:${requestedLibraryId ?? ''}`;

  const targetIsActive = (requestedEnvironment: EnvironmentRef, libraryId: LibraryId) => (
    activeLibraryTarget.current.environmentKey === environmentKey(requestedEnvironment)
    && activeLibraryTarget.current.libraryId === libraryId
  );

  useEffect(() => {
    if (
      committedRouteKeyRef.current === routeSyncKey
      && workspace.selectedLibraryId === requestedLibraryId
    ) return undefined;
    let ignored = false;
    void workspace.execute({ kind: 'load' }).then(async (result) => {
      if (result.status !== 'succeeded') return;
      let loaded = result.snapshot;
      if (
        !ignored
        && requestedLibraryId
        && loaded.catalog?.libraries.some((library) => library.id === requestedLibraryId)
        && loaded.selectedLibraryId !== requestedLibraryId
      ) {
        const selected = await workspace.execute({ kind: 'select', libraryId: requestedLibraryId });
        if (selected.status === 'succeeded') loaded = selected.snapshot;
      }
      if (!ignored) {
        const selectedLibraryId = loaded.selectedLibraryId ?? null;
        const committedKey = `${environmentKey(environment)}:${selectedLibraryId ?? ''}`;
        committedRouteKeyRef.current = committedKey;
        if (requestedLibraryId !== selectedLibraryId) {
          setSearchParams((current) => {
            const next = new URLSearchParams(current);
            if (selectedLibraryId) next.set('library', selectedLibraryId);
            else next.delete('library');
            return next;
          }, { replace: true });
        }
      }
    });
    return () => { ignored = true; };
  }, [environment, requestedLibraryId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (workspace.phase !== 'ready' || workspace.catalog === null) return;
    if (committedRouteKeyRef.current !== routeSyncKey) return;
    if (requestedLibraryId === workspace.selectedLibraryId) return;
    const nextRouteKey = `${environmentKey(environment)}:${workspace.selectedLibraryId ?? ''}`;
    committedRouteKeyRef.current = nextRouteKey;
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      if (workspace.selectedLibraryId) next.set('library', workspace.selectedLibraryId);
      else next.delete('library');
      return next;
    }, { replace: true });
  }, [environment, requestedLibraryId, routeSyncKey, setSearchParams, workspace.catalog, workspace.phase, workspace.selectedLibraryId]);

  useEffect(() => {
    setPageError(null);
    setQuery('');
    setSelectedSkillName(null);
    setSkillContent(null);
    setContentError(false);
    activateUpdate(environment, workspace.selectedLibraryId);
    maintenanceRequestId.current += 1;
    contentRequestId.current += 1;
  }, [activateUpdate, environment, workspace.selectedLibraryId]);

  useEffect(() => () => resetUpdates(), [resetUpdates]);

  const updateBusy = updatePhase === 'checking'
    || updatePhase === 'preparing'
    || updatePhase === 'executing';
  const busy = writeBlocked || workspace.phase === 'writing' || maintenanceBusy || updateBusy;

  const submitName = async () => {
    if (!nameDialog || !name.trim()) return;
    setNameError(null);
    let result;
    if (nameDialog.mode === 'create') {
      result = await workspace.execute({ kind: 'create', name });
    } else if (nameDialog.id) {
      result = await workspace.execute({ kind: 'rename', libraryId: nameDialog.id, name });
    } else return;
    if (result.status === 'failed') {
      setNameError(result.error);
      return;
    }
    if (result.status === 'notRun') return;
    setNameDialog(null);
    setName('');
  };

  const openAddDialog = (trigger: HTMLButtonElement) => {
    const detail = workspace.detail;
    if (!detail || !workspace.selectedLibraryId || busy || libraryInUse) return;
    addTriggerRef.current = trigger;
    const environmentEntry = environments.find(
      (entry) => environmentKey(entry.environment) === environmentKey(environment),
    );
    const environmentName = environmentEntry
      ? environmentDisplayName(environmentEntry, t)
      : environment.kind === 'wsl'
        ? t('context.environmentWslName', { environment: environment.distro_name })
        : t('libraries.addFlow.environment.native');
    setAddRequest({
      target: {
        environment,
        environmentName,
        libraryId: workspace.selectedLibraryId,
        libraryName: detail.name,
      },
      existingSkillNames: new Set(detail.skills.map((skill) => skill.name)),
      execute: workspace.execute,
    });
  };

  const openSkillContent = async (skillName: string) => {
    const libraryId = workspace.selectedLibraryId;
    if (!libraryId) return;
    const requestId = ++contentRequestId.current;
    setSelectedSkillName(skillName);
    setSkillContent(null);
    setContentError(false);
    try {
      const content = await readLibrarySkillContent(environment, libraryId, skillName);
      if (requestId === contentRequestId.current) setSkillContent(content);
    } catch {
      if (requestId === contentRequestId.current) setContentError(true);
    }
  };

  const applyUpdates = async () => {
    const response = await confirmUpdates();
    if (response) {
      await workspace.execute({ kind: 'select', libraryId: response.library.id });
    }
  };

  const submitSkillRemoval = async () => {
    const libraryId = workspace.selectedLibraryId;
    const requestedSkillName = removeSkillName;
    const requestedEnvironment = environment;
    if (!libraryId || !requestedSkillName) return;
    const requestId = ++maintenanceRequestId.current;
    setMaintenanceBusy(true);
    setPageError(null);
    try {
      await removeLibrarySkill({
        environment: requestedEnvironment,
        libraryId,
        skillName: requestedSkillName,
      });
      if (requestId !== maintenanceRequestId.current || !targetIsActive(requestedEnvironment, libraryId)) return;
      await workspace.execute({ kind: 'select', libraryId });
      if (requestId !== maintenanceRequestId.current || !targetIsActive(requestedEnvironment, libraryId)) return;
      setRemoveSkillName(null);
      if (selectedSkillName === requestedSkillName) {
        setSelectedSkillName(null);
      }
    } catch (error) {
      if (requestId === maintenanceRequestId.current) setPageError({ scope: 'remove', error: error as AppError });
    } finally {
      if (requestId === maintenanceRequestId.current) setMaintenanceBusy(false);
    }
  };

  const availableUpdates = useMemo(() => (
    Object.values(checks)
      .filter((check) => check.status === 'updateAvailable')
      .map((check) => check.name)
  ), [checks]);

  const updateSummaryItems = useMemo(
    () => formatLibraryUpdateSummaryItems(summarizeLibraryUpdates(checks, updateError), t),
    [checks, updateError, t],
  );
  const updateSummary = updateSummaryItems.map((item) => item.text).join(' · ');
  const firstWarningSummaryIndex = updateSummaryItems.findIndex((item) => item.tone === 'warning');

  // 批次内的成员共享当前阶段，卡片据此显示进度条；与 Skills 页同一模型。
  const memberUpdateStatuses = useMemo(
    () => libraryUpdateDisplayStatuses(updatePhase, pendingUpdate?.request.skillNames ?? [], lastResults),
    [updatePhase, pendingUpdate, lastResults],
  );

  const libraryInUse = (workspace.detail?.usages.length ?? 0) > 0;
  // 与 Skills 页一致：选中成员即进入分栏，列表切换为紧凑导航。
  const compact = Boolean(selectedSkillName);

  // 检查完成且没有发现更新时，按钮就地换成 ✓，800ms 后恢复——与 Skills 页同一反馈。
  const [checkDone, setCheckDone] = useState(false);
  const hideCheckDoneTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (hideCheckDoneTimerRef.current) clearTimeout(hideCheckDoneTimerRef.current);
  }, []);

  const handleCheckUpdates = useCallback(async () => {
    await checkUpdates();
    const state = useLibraryUpdateWorkflow.getState();
    const foundUpdates = Object.values(state.checks)
      .some((check) => check.status === 'updateAvailable');
    if (state.hasError || foundUpdates) return;
    if (hideCheckDoneTimerRef.current) clearTimeout(hideCheckDoneTimerRef.current);
    setCheckDone(true);
    hideCheckDoneTimerRef.current = setTimeout(() => {
      setCheckDone(false);
      hideCheckDoneTimerRef.current = null;
    }, 800);
  }, [checkUpdates]);

  // 次要维护操作成组，样式与 Skills 页区域头一致：低调的 ghost，hover 才转主色。
  const libraryActions = (
    <div className="flex items-center gap-0.5">
      {availableUpdates.length > 0 ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="library-secondary-action h-7 cursor-pointer gap-1.5 px-2 text-xs font-medium text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary"
              onClick={() => void prepareUpdates(availableUpdates)}
              disabled={busy}
              aria-label={t('libraries.updateCount', { count: availableUpdates.length })}
            >
              <ArrowUpCircle className="size-3.5 shrink-0" aria-hidden="true" />
              <span className="library-secondary-action-label">
                {t('libraries.updateCount', { count: availableUpdates.length })}
              </span>
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('libraries.updateCount', { count: availableUpdates.length })}</TooltipContent>
        </Tooltip>
      ) : null}

      {(workspace.detail?.skills.length ?? 0) > 0 ? (
        checkDone && availableUpdates.length === 0 ? (
          <span className="inline-flex h-7 items-center justify-center gap-1.5 px-2 text-xs font-medium text-success">
            <Check className="size-3.5" aria-hidden="true" />
            <span className="library-secondary-action-label">{t('skills.checkCompleted')}</span>
          </span>
        ) : (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="library-secondary-action h-7 cursor-pointer gap-1.5 px-2 text-xs font-medium text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary"
                onClick={() => void handleCheckUpdates()}
                disabled={busy}
                aria-busy={updatePhase === 'checking'}
                aria-label={t('libraries.checkUpdates')}
              >
                <RefreshCw
                  className={cn('size-3.5 shrink-0', updatePhase === 'checking' && 'animate-spin')}
                  aria-hidden="true"
                />
                <span className="library-secondary-action-label">{t('libraries.checkUpdates')}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t('libraries.checkUpdates')}</TooltipContent>
          </Tooltip>
        )
      ) : null}
    </div>
  );
  const visibleSkills = useMemo(() => {
    return workspace.detail?.skills.filter((skill) => {
      const normalized = query.trim().toLocaleLowerCase();
      return !normalized || [skill.name, skill.description, skill.source]
        .some((value) => value.toLocaleLowerCase().includes(normalized));
    }) ?? [];
  }, [workspace.detail?.skills, query]);

  const selectedSkill = useMemo(() => {
    if (!selectedSkillName || !workspace.detail) return null;
    return workspace.detail.skills.find((s) => s.name === selectedSkillName) ?? null;
  }, [selectedSkillName, workspace.detail]);

  // 分栏出现或消失时重置布局。单栏时列表必须能占满 100%，否则会被 maxSize 钳住留白。
  useLayoutEffect(() => {
    const hasDetail = Boolean(selectedSkill);
    if (hasDetail === previousSplitViewRef.current) return;

    const expectedPanelCount = hasDetail ? 2 : 1;
    const nextLayout = hasDetail ? SPLIT_VIEW_LAYOUT : LIST_VIEW_LAYOUT;
    let cancelled = false;

    const applyLayoutWhenReady = (attempt = 0) => {
      if (cancelled) return;
      const group = layoutRef.current;
      if (!group) return;

      if (Object.keys(group.getLayout()).length === expectedPanelCount) {
        group.setLayout(nextLayout);
        previousSplitViewRef.current = hasDetail;
        return;
      }
      if (attempt < 10) queueMicrotask(() => applyLayoutWhenReady(attempt + 1));
    };

    applyLayoutWhenReady();
    return () => { cancelled = true; };
  }, [selectedSkill, layoutRef]);

  return (
    <TooltipProvider>
      <div className="library-page-shell flex h-full min-h-0 w-full overflow-hidden bg-background">
        {/* 左侧侧边栏 */}
        <LibrarySidebar
          libraries={workspace.catalog?.libraries ?? []}
          usageProjection={workspace.catalog?.usageProjection}
          selectedLibraryId={workspace.selectedLibraryId}
          busy={busy}
          onSelectLibrary={(id) => void selectLibrary(id)}
          onCreateLibrary={() => {
            setName('');
            setNameError(null);
            setNameDialog({ mode: 'create' });
          }}
          onRenameLibrary={(lib: SkillLibrarySummary) => {
            setName(lib.name);
            setNameError(null);
            setNameDialog({ mode: 'rename', id: lib.id });
          }}
          onDeleteLibrary={(library) => {
            setDeleteRequest(captureLibraryDeletion(environment, library));
          }}
        />

      {/* 右侧主工作区 */}
      <main className="library-workspace flex min-w-0 flex-1 flex-col overflow-hidden bg-panel">
        {workspace.catalogError && workspace.catalog ? (
          <div role="alert" className="library-workspace-gutter flex shrink-0 items-center justify-between gap-3 py-2 text-sm text-destructive">
            <span>{t('libraries.loadError')}</span>
            <Button
              size="sm"
              variant="ghost"
              className="h-7 shrink-0"
              onClick={() => void workspace.execute({ kind: 'load' })}
            >
              <RefreshCw className="size-3.5" aria-hidden="true" />
              {t('common.retry')}
            </Button>
          </div>
        ) : null}
        {workspace.catalogError && !workspace.catalog ? (
          <div role="alert" className="m-auto max-w-md py-16 text-center">
            <p className="text-sm font-medium text-destructive">{t('libraries.loadError')}</p>
            <Button className="mt-4 gap-2" variant="outline" onClick={() => void workspace.execute({ kind: 'load' })}>
              <RefreshCw className="size-4" aria-hidden="true" />
              {t('common.retry')}
            </Button>
          </div>
        ) : workspace.detail ? (
          <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
            {/* 库 header 由主工作区宽度控制。最窄档使用固定双行，避免自由换行导致跳动。 */}
            <header className="library-workspace-header library-workspace-gutter shrink-0 pb-4 pt-3">
              <div className="library-header-identity">
                <h2 className="min-w-0 shrink truncate text-sm font-bold tracking-normal text-foreground/90">
                  <span className="truncate">{workspace.detail.name}</span>
                </h2>
                <span className="shrink-0 text-xs font-medium tabular-nums text-muted-foreground/70">
                  {t('libraries.skillCount', { count: workspace.detail.skills.length })}
                </span>

              </div>

              {/* Skill 维护状态在前，应用关系在后，两个口径不再插入彼此。 */}
              <div className="library-header-facts text-xs text-muted-foreground">
                <span className="shrink-0 text-border" aria-hidden="true">·</span>
                {updateSummary ? (
                  <>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span
                          className="library-update-summary inline-flex min-w-0 items-center gap-1 truncate rounded-sm tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                          role="status"
                          tabIndex={0}
                          aria-label={updateSummary}
                          aria-live="polite"
                          aria-atomic="true"
                        >
                          {updateSummaryItems.map((item, index) => (
                            <span
                              key={`${item.tone}:${item.text}`}
                              className={cn(
                                'inline-flex min-w-0 items-center gap-1 font-medium',
                                item.tone === 'accent' && 'text-primary',
                                item.tone === 'warning' && 'text-warning',
                                item.tone === 'neutral' && 'text-muted-foreground/80',
                              )}
                            >
                              {index > 0 ? (
                                <span className="shrink-0 text-border" aria-hidden="true">·</span>
                              ) : null}
                              {index === firstWarningSummaryIndex ? (
                                <CircleAlert className="size-3.5 shrink-0" aria-hidden="true" />
                              ) : null}
                              <span className="truncate">{item.text}</span>
                            </span>
                          ))}
                        </span>
                      </TooltipTrigger>
                      <TooltipContent>{updateSummary}</TooltipContent>
                    </Tooltip>
                    <span className="shrink-0 text-border" aria-hidden="true">·</span>
                  </>
                ) : null}
                <LibraryUsageLine usages={workspace.detail.usages} />
              </div>

              <div className="library-header-actions flex shrink-0 items-center gap-2">
                {libraryActions}
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      className="h-7 shrink-0 cursor-pointer gap-1.5 border border-transparent bg-primary/[0.04] px-2.5 text-xs font-semibold text-primary/80 shadow-none transition-colors hover:bg-primary/10 hover:text-primary sm:px-3"
                      onClick={(event) => {
                        if (!busy && !libraryInUse) openAddDialog(event.currentTarget);
                      }}
                      aria-disabled={busy || libraryInUse}
                      aria-label={t('libraries.addSkill')}
                    >
                      <Plus className="size-3.5 shrink-0" aria-hidden="true" />
                      <span>{t('libraries.addSkill')}</span>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>
                    <p>{t(libraryInUse ? 'libraries.lockedMembership' : 'libraries.addSkill')}</p>
                  </TooltipContent>
                </Tooltip>
              </div>
            </header>

            {/* 主内容区域：双栏 Split View 或 单栏 */}
            <div className="flex-1 min-h-0 overflow-hidden">
              <ResizablePanelGroup
                id="library-page-split-layout"
                orientation="horizontal"
                className="h-full"
                groupRef={layoutRef}
              >
                {/* 列表 Panel */}
                <ResizablePanel
                  id="library-skills-list-panel"
                  defaultSize={selectedSkill ? '22%' : '100%'}
                  minSize={selectedSkill ? '12%' : '100%'}
                  maxSize={selectedSkill ? '85%' : '100%'}
                  className="h-full flex flex-col min-w-0"
                >
                  {/* 搜索永久属于列表面板：它过滤的就是下面这个列表，紧凑模式下也跟着收窄。
                      放在 ScrollArea 之外，列表滚动时不会跑掉。 */}
                  {workspace.detail.skills.length > 0 ? (
                    <div className="library-workspace-gutter shrink-0 pb-2">
                      <div className="relative">
                        <Search
                          className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
                          aria-hidden="true"
                        />
                        <Input
                          type="search"
                          name="library-skill-search"
                          autoComplete="off"
                          spellCheck={false}
                          value={query}
                          onChange={(event) => setQuery(event.target.value)}
                          placeholder={t('libraries.searchSkills')}
                          aria-label={t('libraries.searchSkills')}
                          className="h-8 pl-9 pr-8"
                        />
                        {query ? (
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="absolute right-1 top-1/2 size-6 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                            onClick={() => setQuery('')}
                            aria-label={t('common.clear')}
                          >
                            <X className="size-3.5" aria-hidden="true" />
                          </Button>
                        ) : null}
                      </div>
                      {query.trim() ? (
                        <p
                          className="mt-1 text-right text-[11px] tabular-nums text-muted-foreground"
                          aria-live="polite"
                        >
                          {t('libraries.searchResultCount', {
                            visible: visibleSkills.length,
                            total: workspace.detail.skills.length,
                          })}
                        </p>
                      ) : null}
                    </div>
                  ) : null}

                  <ScrollArea className="h-full flex-1">
                    <div className={cn(
                      'w-full',
                      // 与 header 共用左边缘：居中限宽会让卡片和库名对不齐。
                      compact ? 'py-1' : 'library-workspace-gutter space-y-3 pb-4 pt-1',
                    )}>
                      {pageError?.scope === 'remove' ? (
                        <div role="alert" className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-sm text-destructive">
                          {formatAppError(pageError.error, t)}
                        </div>
                      ) : null}

                      {/* Skill 卡片列表 */}
                      {workspace.detail.skills.length === 0 ? (
                        <div className="flex flex-col items-center justify-center py-16 text-center">
                          <div className="mb-3 flex size-12 items-center justify-center rounded-full bg-muted">
                            <BookOpen className="size-6 text-muted-foreground" aria-hidden="true" />
                          </div>
                          <p className="text-sm font-medium text-foreground">{t('libraries.empty')}</p>
                          <Button
                            size="sm"
                            className="mt-4 gap-1.5 shadow-xs"
                            onClick={(event) => openAddDialog(event.currentTarget)}
                            disabled={busy || libraryInUse}
                          >
                            <Plus className="size-4" aria-hidden="true" />
                            {t('libraries.addSkill')}
                          </Button>
                        </div>
                      ) : visibleSkills.length === 0 ? (
                        <p className="py-12 text-center text-sm text-muted-foreground">
                          {t('libraries.noSearchResults')}
                        </p>
                      ) : compact ? (
                        // 选中成员后列表退居为导航：操作与完整信息都归详情面板。
                        <div>
                          {visibleSkills.map((skill) => (
                            <LibraryCompactItem
                              key={skill.name}
                              skill={skill}
                              isSelected={selectedSkillName === skill.name}
                              onClick={(name) => void openSkillContent(name)}
                            />
                          ))}
                        </div>
                      ) : (
                        <div className="space-y-3">
                          {visibleSkills.map((skill) => (
                            <LibrarySkillCard
                              key={skill.name}
                              skill={skill}
                              check={checks[skill.name]}
                              updateStatus={memberUpdateStatuses[skill.name]}
                              busy={busy}
                              libraryInUse={libraryInUse}
                              onClick={(name) => void openSkillContent(name)}
                              onUpdate={(name) => void prepareUpdates([name])}
                              onRemove={(name) => setRemoveSkillName(name)}
                            />
                          ))}
                        </div>
                      )}
                    </div>
                  </ScrollArea>
                </ResizablePanel>

                {/* 详情 Panel (仅当选中 Skill 时展示) */}
                {selectedSkill ? (
                  <>
                    <ResizableHandle className="bg-border/60 hover:bg-primary transition-colors" />
                    <ResizablePanel
                      id="library-skill-detail-panel"
                      defaultSize="78%"
                      minSize="15%"
                      className="h-full bg-surface"
                    >
                      <LibrarySkillDetailPanel
                        skill={selectedSkill}
                        check={checks[selectedSkill.name]}
                        content={skillContent}
                        loading={skillContent === null && !contentError}
                        contentError={contentError}
                        busy={busy}
                        libraryInUse={libraryInUse}
                        onClose={() => setSelectedSkillName(null)}
                        onUpdate={(name) => void prepareUpdates([name])}
                        onRemove={(name) => setRemoveSkillName(name)}
                        onRetry={() => void openSkillContent(selectedSkill.name)}
                      />
                    </ResizablePanel>
                  </>
                ) : null}
              </ResizablePanelGroup>
            </div>
          </div>
        ) : workspace.selectedLibraryId && workspace.detailPhase === 'error' ? (
          <div className="m-auto max-w-md py-16 text-center">
            <p className="text-sm font-medium text-destructive">{t('libraries.detailLoadError')}</p>
            <Button
              className="mt-4 gap-2"
              variant="outline"
              onClick={() => void workspace.execute({
                kind: 'select',
                libraryId: workspace.selectedLibraryId!,
              })}
            >
              <RefreshCw className="size-4" aria-hidden="true" />
              {t('common.retry')}
            </Button>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col items-center justify-center text-center p-6 text-muted-foreground">
            <div className="mb-4 flex size-14 items-center justify-center rounded-2xl bg-muted/60 ring-1 ring-border">
              <Library className="size-7 text-muted-foreground" aria-hidden="true" />
            </div>
            <p className="text-sm font-medium text-foreground">{t('libraries.noLibraries')}</p>
            <Button
              className="mt-4 gap-1.5 shadow-xs"
              onClick={() => {
                setName('');
                setNameError(null);
                setNameDialog({ mode: 'create' });
              }}
            >
              <Plus className="size-4" aria-hidden="true" />
              {t('libraries.create')}
            </Button>
          </div>
        )}
      </main>

      {/* 创建 / 重命名库 Dialog */}
      <Dialog open={nameDialog !== null} onOpenChange={(open) => {
        if (!open) {
          setNameError(null);
          setNameDialog(null);
        }
      }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t(nameDialog?.mode === 'rename' ? 'libraries.rename' : 'libraries.create')}</DialogTitle>
            <DialogDescription className="sr-only">{t('libraries.nameDialogDescription')}</DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-2">
            <Label htmlFor="library-name">{t('libraries.name')}</Label>
            <Input
              id="library-name"
              name="library-name"
              autoComplete="off"
              value={name}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void submitName();
              }}
            />
            {nameError ? (
              <p role="alert" className="text-sm text-destructive">
                {formatAppError(nameError, t)}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => {
              setNameError(null);
              setNameDialog(null);
            }}>{t('common.cancel')}</Button>
            <Button type="button" onClick={() => void submitName()} disabled={busy || !name.trim()}>{t('common.save')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 确认更新 Dialog */}
      <Dialog open={pendingUpdate !== null} onOpenChange={(open) => {
        if (!open) {
          cancelUpdates();
        }
      }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('libraries.confirmUpdateTitle')}</DialogTitle>
            <DialogDescription>{t('libraries.confirmUpdateDescription', { count: pendingUpdate?.request.skillNames.length ?? 0 })}</DialogDescription>
            {(pendingUpdate?.redirectedDownloadHosts.length ?? 0) > 0 ? (
              <p className="text-sm text-warning">
                {t('libraries.redirectConfirmation', { host: pendingUpdate?.redirectedDownloadHosts.join(', ') })}
              </p>
            ) : null}
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => {
              cancelUpdates();
            }}>{t('common.cancel')}</Button>
            <Button type="button" onClick={() => void applyUpdates()} disabled={busy}>{t('libraries.update')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 移除 Skill Dialog */}
      <Dialog open={removeSkillName !== null} onOpenChange={(open) => !open && setRemoveSkillName(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('libraries.removeSkillTitle', { name: removeSkillName ?? '' })}</DialogTitle>
            <DialogDescription>{t('libraries.removeSkillDescription')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setRemoveSkillName(null)}>{t('common.cancel')}</Button>
            <Button type="button" variant="destructive" onClick={() => void submitSkillRemoval()} disabled={busy}>{t('common.delete')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <DeleteLibraryDialog request={deleteRequest} onClose={() => setDeleteRequest(null)} />
      {addRequest ? (
        <LibraryAddDialog
          open
          target={addRequest.target}
          existingSkillNames={addRequest.existingSkillNames}
          execute={addRequest.execute}
          onClose={() => {
            setAddRequest(null);
            queueMicrotask(() => addTriggerRef.current?.focus({ preventScroll: true }));
          }}
        />
      ) : null}
    </div>    </TooltipProvider>  );
}
