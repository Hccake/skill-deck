import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useGroupRef, type Layout } from 'react-resizable-panels';
import { PanelLeft, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { resolveSkillsLayout, SKILL_DETAIL_MIN_WIDTH, SKILL_DETAIL_COMFORT_WIDTH, SKILLS_LIST_MIN_WIDTH, SKILLS_SIDEBAR_MIN_WIDTH } from '@/lib/skills-layout';
import { useLayoutStore } from '@/stores/layout';
import { cn } from '@/lib/utils';

const PORTAL_SURFACE = '[role="dialog"], [role="alertdialog"], [role="menu"], [data-radix-focus-guard]';

interface SkillsWorkspaceProps {
  kind?: 'skills' | 'library';
  contextKey: string;
  sidebar: ReactNode;
  children: ReactNode | ((navigation: ReactNode) => ReactNode);
  detailOpen: boolean;
  onCloseDetail: () => void;
  detail: ReactNode;
  header?: (navigation: ReactNode) => ReactNode;
}

export function SkillsWorkspace({ kind = 'skills', contextKey, sidebar, children, detailOpen, onCloseDetail, detail, header }: SkillsWorkspaceProps) {
  const { t } = useTranslation();
  const hasDetail = detailOpen;
  const preferredWidth = useLayoutStore((state) => kind === 'skills' ? state.skillsListWidth : state.libraryListWidth);
  const saveWidth = useLayoutStore((state) => kind === 'skills' ? state.setSkillsListWidth : state.setLibraryListWidth);
  const preferredSidebarWidth = useLayoutStore((state) => kind === 'skills' ? state.skillsSidebarWidth : state.librarySidebarWidth);
  const saveSidebarWidth = useLayoutStore((state) => kind === 'skills' ? state.setSkillsSidebarWidth : state.setLibrarySidebarWidth);
  const listId = kind === 'skills' ? 'skills-list-panel' : 'library-skills-list-panel';
  const detailId = kind === 'skills' ? 'skill-detail-panel' : 'library-skill-detail-panel';
  const navigationId = `${kind}-context-navigation`;
  const navigationPanelId = `${kind}-navigation-panel`;
  const contentPanelId = `${kind}-content-panel`;
  const navigationLabel = t(kind === 'skills' ? 'context.navigation' : 'libraries.navigation');
  const shellRef = useRef<HTMLDivElement>(null);
  const navigationRef = useRef<HTMLDivElement>(null);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const groupRef = useGroupRef();
  const navigationGroupRef = useGroupRef();
  const sidebarHandleRef = useRef<HTMLDivElement>(null);
  const sidebarHandleFocused = useRef(false);
  const pendingLayout = useRef(true);
  const pendingSidebarLayout = useRef(true);
  const userResizing = useRef(false);
  const userResizingSidebar = useRef(false);
  const toggleFocused = useRef(false);
  const [viewport, setViewport] = useState({ width: window.innerWidth, collapsed: false });
  const [openContext, setOpenContext] = useState<string | null>(null);
  const { sidebarWidth, sidebarMaxWidth, listWidth, navigationCollapsed } = resolveSkillsLayout(
    viewport.width, hasDetail, preferredWidth, viewport.collapsed, preferredSidebarWidth,
  );
  const navigationOpen = navigationCollapsed && openContext === contextKey;
  const availableWidth = viewport.width - sidebarWidth - 1;

  useLayoutEffect(() => {
    const element = shellRef.current;
    if (!element) return;
    const measure = () => {
      const width = element.getBoundingClientRect().width;
      if (width <= 0) return;
      setViewport((previous) => {
        const collapsed = resolveSkillsLayout(width, hasDetail, preferredWidth, previous.collapsed, preferredSidebarWidth).navigationCollapsed;
        return width === previous.width && collapsed === previous.collapsed ? previous : { width, collapsed };
      });
      if (!resolveSkillsLayout(width, hasDetail, preferredWidth, true, preferredSidebarWidth).navigationCollapsed) setOpenContext(null);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    measure();
    return () => observer.disconnect();
  }, [hasDetail, preferredWidth, preferredSidebarWidth]);

  const applySidebarLayout = useCallback(() => {
    const group = navigationGroupRef.current;
    if (!pendingSidebarLayout.current || !group || viewport.width <= 0) return;
    if (Object.keys(group.getLayout()).length !== 2) return;
    pendingSidebarLayout.current = false;
    userResizingSidebar.current = false;
    const percentage = sidebarWidth / viewport.width * 100;
    const applied = group.setLayout({ [navigationPanelId]: percentage, [contentPanelId]: 100 - percentage });
    pendingSidebarLayout.current = Math.abs(applied[navigationPanelId] - percentage) > 0.01;
  }, [contentPanelId, navigationGroupRef, navigationPanelId, sidebarWidth, viewport.width]);

  useLayoutEffect(() => {
    pendingSidebarLayout.current = true;
    applySidebarLayout();
  }, [applySidebarLayout]);

  const applyLayout = useCallback(() => {
    const group = groupRef.current;
    if (!pendingLayout.current || !group || availableWidth <= 0) return;
    if (Object.keys(group.getLayout()).length !== (hasDetail ? 2 : 1)) return;
    pendingLayout.current = false;
    userResizing.current = false;
    // 工作区负责像素偏好；分栏库保持默认比例策略，避免侧栏变化时重复换算。
    const percentage = listWidth / availableWidth * 100;
    const applied = group.setLayout(hasDetail ? {
      [listId]: percentage,
      [detailId]: 100 - percentage,
    } : { [listId]: 100 });
    // 约束尚未随容器更新时结果可能被夹紧，等实际尺寸回调后再应用。
    pendingLayout.current = hasDetail && Math.abs(applied[listId] - percentage) > 0.01;
  }, [availableWidth, detailId, groupRef, hasDetail, listId, listWidth]);

  useLayoutEffect(() => {
    pendingLayout.current = true;
    applyLayout();
  }, [applyLayout]);

  useLayoutEffect(() => {
    const navigation = navigationRef.current;
    if (navigationCollapsed && !navigationOpen && (navigation?.contains(document.activeElement) || sidebarHandleFocused.current)) {
      sidebarHandleFocused.current = false;
      toggleRef.current?.focus();
    } else if (navigationOpen || (!navigationCollapsed && toggleFocused.current)) {
      const current = navigation?.querySelector<HTMLElement>('[aria-current="location"]')
        ?? navigation?.querySelector<HTMLElement>('button:not(:disabled)');
      current?.focus();
      toggleFocused.current = false;
    }
  }, [navigationCollapsed, navigationOpen]);

  const closeNavigation = () => {
    setOpenContext(null);
    toggleRef.current?.focus();
  };

  const rememberUserLayout = (layout: Layout) => {
    if (!userResizing.current || pendingLayout.current || !hasDetail) return;
    userResizing.current = false;
    saveWidth(layout[listId] / 100 * availableWidth);
  };

  const rememberSidebarLayout = (layout: Layout) => {
    if (!userResizingSidebar.current || pendingSidebarLayout.current || navigationCollapsed) return;
    userResizingSidebar.current = false;
    saveSidebarWidth(layout[navigationPanelId] / 100 * viewport.width);
  };

  const navigationToggle = navigationCollapsed ? (
    <Button
      ref={toggleRef}
      variant="ghost"
      size="icon-sm"
      className="size-7 shrink-0"
      aria-label={navigationLabel}
      title={navigationLabel}
      aria-expanded={navigationOpen}
      aria-controls={navigationId}
      onFocus={() => { toggleFocused.current = true; }}
      onBlur={() => { toggleFocused.current = false; }}
      onClick={() => navigationOpen ? closeNavigation() : setOpenContext(contextKey)}
    >
      <PanelLeft className="size-4" aria-hidden="true" />
    </Button>
  ) : null;

  return (
    <div
      ref={shellRef}
      className={cn(kind === 'skills' ? 'skills-page-shell' : 'library-page-shell', 'relative flex h-full min-h-0 min-w-0 overflow-hidden')}
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || event.defaultPrevented || !detailOpen) return;
        if (event.target instanceof Element && event.target.closest(PORTAL_SURFACE)) return;
        event.preventDefault();
        event.stopPropagation();
        onCloseDetail();
      }}
    >
      {navigationOpen ? (
        <button type="button" tabIndex={-1} className="absolute inset-0 z-20 bg-black/15" aria-label={t('common.close')} onClick={closeNavigation} />
      ) : null}
      <ResizablePanelGroup
        id={`${kind}-navigation-layout`}
        orientation="horizontal"
        groupRef={navigationGroupRef}
        onLayoutChange={applySidebarLayout}
        onLayoutChanged={rememberSidebarLayout}
        onPointerDownCapture={(event) => {
          userResizingSidebar.current = event.defaultPrevented && document.activeElement === sidebarHandleRef.current;
        }}
      >
      <ResizablePanel
        id={navigationPanelId}
        defaultSize={sidebarWidth}
        minSize={navigationCollapsed ? '0%' : SKILLS_SIDEBAR_MIN_WIDTH}
        maxSize={navigationCollapsed ? '0%' : sidebarMaxWidth}
        onResize={applySidebarLayout}
        style={{ overflow: 'visible' }}
      >
      <div
        id={navigationId}
        ref={navigationRef}
        role="navigation"
        aria-label={navigationLabel}
        hidden={navigationCollapsed && !navigationOpen}
        className={navigationCollapsed ? 'absolute inset-y-0 left-0 z-30 w-52 shadow-lg' : 'h-full w-full'}
        onBlur={(event) => {
          const next = event.relatedTarget;
          if (navigationOpen && next instanceof Element && !event.currentTarget.contains(next) && !next.closest(PORTAL_SURFACE)) setOpenContext(null);
        }}
        onKeyDown={(event) => {
          if (event.key !== 'Escape' || event.defaultPrevented || !navigationOpen) return;
          if (event.target instanceof Element && event.target.closest(PORTAL_SURFACE)) return;
          event.preventDefault();
          event.stopPropagation();
          closeNavigation();
        }}
      >
        {sidebar}
      </div>
      </ResizablePanel>
      <ResizableHandle
        id={`${kind}-sidebar-resize`}
        elementRef={sidebarHandleRef}
        aria-label={t('skills.resizeSidebar')}
        title={t('skills.resizeSidebar')}
        disabled={navigationCollapsed}
        hidden={navigationCollapsed}
        onFocus={() => { sidebarHandleFocused.current = true; }}
        onBlur={(event) => {
          // 隐藏分隔条可能先让焦点回到页面，保留标记供抽屉入口接管。
          if (event.relatedTarget instanceof Element && event.relatedTarget !== document.body) sidebarHandleFocused.current = false;
        }}
        className={cn('w-0 before:absolute before:inset-y-0 before:left-0 before:w-px hover:before:bg-primary/60', navigationCollapsed && 'hidden')}
        onKeyDownCapture={(event) => {
          if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) userResizingSidebar.current = true;
        }}
      />
      <ResizablePanel
        id={contentPanelId}
        defaultSize={viewport.width - sidebarWidth}
        minSize={navigationCollapsed ? '100%' : hasDetail ? SKILLS_LIST_MIN_WIDTH + SKILL_DETAIL_COMFORT_WIDTH + 1 : SKILLS_LIST_MIN_WIDTH}
        className={cn('flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-panel', kind === 'library' && 'library-workspace')}
      >
        {header?.(navigationToggle)}
        <div className="min-h-0 flex-1">
        <ResizablePanelGroup
          id={kind === 'skills' ? 'skills-page-layout' : 'library-page-split-layout'}
          orientation="horizontal"
          groupRef={groupRef}
          onLayoutChange={applyLayout}
          onLayoutChanged={rememberUserLayout}
          onPointerDownCapture={(event) => {
            // 扩大的拖动命中区可以落在相邻面板上；分栏库接管操作时会聚焦分隔条并消费事件。
            userResizing.current = event.defaultPrevented
              && document.activeElement === event.currentTarget.querySelector('[role="separator"]');
          }}
        >
          <ResizablePanel
            id={listId}
            defaultSize={hasDetail ? listWidth : '100%'}
            minSize={hasDetail ? SKILLS_LIST_MIN_WIDTH : '100%'}
            onResize={applyLayout}
            className="flex h-full min-h-0 flex-col"
          >
            {typeof children === 'function' ? children(navigationToggle) : children}
          </ResizablePanel>
          {hasDetail ? <>
            <ResizableHandle
              id={`${kind}-detail-resize`}
              aria-label={t('skills.resizePanels')}
              className="hover:bg-primary/60 transition-colors"
              onKeyDownCapture={(event) => {
                if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) userResizing.current = true;
              }}
            />
            <ResizablePanel id={detailId} minSize={SKILL_DETAIL_MIN_WIDTH} className="bg-surface">
              {detail ?? (
                <div className="flex h-full flex-col">
                  <div className="flex justify-end px-6 py-4 sm:px-8">
                    <Button variant="ghost" size="icon-sm" aria-label={t('common.close')} title={t('common.close')} onClick={onCloseDetail}>
                      <X className="size-4" aria-hidden="true" />
                    </Button>
                  </div>
                  <div role="status" className="flex flex-1 items-center justify-center px-6 pb-16 text-sm text-muted-foreground">
                    {t('skills.noSelection')}
                  </div>
                </div>
              )}
            </ResizablePanel>
          </> : null}
        </ResizablePanelGroup>
        </div>
      </ResizablePanel>
      </ResizablePanelGroup>
    </div>
  );
}
