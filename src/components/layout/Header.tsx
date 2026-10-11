import { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { matchPath, NavLink, useLocation, useNavigate } from 'react-router-dom';
import { Sun, Moon, Package, Settings, Check, Compass, LibraryBig } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { OverflowTooltip } from '@/components/ui/overflow-tooltip';
import { RecoveryCenter } from '@/components/recovery/RecoveryCenter';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useSettingsStore } from '@/stores/settings';
import { cn } from '@/lib/utils';
import { isMac, isWindows } from '@/lib/platform';
import type { Locale } from '@/stores/settings';
import logoUrl from '@/assets/logo.png';
import { useOptionalUnsavedChanges } from '@/lifecycle/unsaved-changes-context';
import { GlobalEnvironmentSwitcher } from './GlobalEnvironmentSwitcher';
import { InstallWizardStatusControl } from './InstallWizardStatusControl';
import { WindowControls } from './WindowControls';

// Hoisted outside component to avoid recreation on each render
const getNavLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'group relative flex h-8 shrink-0 items-center gap-1.5 px-2.5 sm:px-3 rounded text-sm leading-5 font-medium tracking-normal transition-colors duration-150 select-none hover:bg-foreground/5 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
    isActive
      ? "text-foreground font-semibold after:pointer-events-none after:absolute after:bottom-0.5 after:left-1/2 after:h-0.5 after:w-5 after:-translate-x-1/2 after:rounded-full after:bg-primary after:content-['']"
      : 'text-zinc-600 dark:text-muted-foreground hover:text-foreground'
  );

const LOCALE_OPTIONS: { value: Locale; code: string; label: string }[] = [
  { value: 'zh-CN', code: 'ZH', label: '简体中文' },
  { value: 'en', code: 'EN', label: 'English' },
];

const NAV_ITEMS = [
  { to: '/', label: 'nav.skills', icon: Package },
  { to: '/libraries', label: 'nav.libraries', icon: LibraryBig },
  { to: '/discover', label: 'nav.discover', icon: Compass },
  { to: '/settings', label: 'nav.settings', icon: Settings },
];

export function Header() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const unsavedChanges = useOptionalUnsavedChanges();
  const theme = useSettingsStore((state) => state.theme);
  const toggleTheme = useSettingsStore((state) => state.toggleTheme);
  const locale = useSettingsStore((state) => state.locale);
  const setLocale = useSettingsStore((state) => state.setLocale);
  const guardNavigation = (event: React.MouseEvent, target: string) => {
    if (!unsavedChanges) return;
    event.preventDefault();
    void unsavedChanges.guard(() => navigate(target));
  };
  const mac = isMac();
  const windows = isWindows();
  const navigationSpace = useRef<HTMLDivElement>(null);
  const navigation = useRef<HTMLElement>(null);
  const [showLabels, setShowLabels] = useState(true);

  useLayoutEffect(() => {
    const space = navigationSpace.current;
    const nav = navigation.current;
    const header = space?.parentElement;
    if (!space || !nav || !header) return;
    let disposed = false;
    const measure = () => {
      const available = space.getBoundingClientRect().width;
      if (disposed || available <= 0) return;
      const current = nav.querySelector('[aria-current="page"]');
      const gap = current ? parseFloat(getComputedStyle(current).columnGap) || 0 : 6;
      // 收起的文字仍保留自然宽度用于测量，切换显示状态不会改变所需总宽度。
      let fullWidth = nav.getBoundingClientRect().width;
      for (const label of nav.querySelectorAll<HTMLElement>('[data-nav-label]')) {
        if (label.parentElement!.getBoundingClientRect().width === 0) {
          fullWidth += label.getBoundingClientRect().width + gap;
        }
      }
      // 窄窗口使用紧凑导航；展开时两侧各保留 24px 拖动空白，并留出恢复缓冲。
      const wideEnough = header.getBoundingClientRect().width >= 800;
      setShowLabels((previous) => wideEnough && fullWidth + 48 + (previous ? 0 : 8) <= available);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(header);
    observer.observe(space);
    observer.observe(nav);
    for (const label of nav.querySelectorAll('[data-nav-label]')) observer.observe(label);
    measure();
    void document.fonts?.ready.then(measure);
    return () => { disposed = true; observer.disconnect(); };
  }, [locale]);

  return (
    <header
      data-tauri-drag-region
      className="flex h-12 items-center border-b border-border/60 bg-background shrink-0 gap-2 overflow-hidden select-none"
    >
      {/* Left: Logo, Title & Environment */}
      <div
        className={cn(
          'flex shrink-0 items-center gap-2 min-w-0',
          mac ? 'pl-[76px]' : 'pl-2 sm:pl-3',
        )}
        data-tauri-drag-region
      >
        <div className="flex items-center gap-2 shrink-0" data-tauri-drag-region="false">
          <img src={logoUrl} alt="Logo" className="h-5 w-5 object-contain" />
          <span className="font-semibold text-sm tracking-normal text-foreground/90 hidden sm:inline select-none truncate">
            {t('app.name')}
          </span>
        </div>
        <div className="min-w-0" data-tauri-drag-region="false">
          <GlobalEnvironmentSwitcher />
        </div>
      </div>

      {/* 一级导航融入标题栏，项目之间的空隙保持可拖动。 */}
      <div ref={navigationSpace} className="flex h-full min-w-0 flex-1 items-center justify-center" data-tauri-drag-region>
        <nav
          ref={navigation}
          className="flex w-max shrink-0 items-center gap-1"
          data-tauri-drag-region
        >
          {NAV_ITEMS.map(({ to, label, icon: Icon }) => {
            const active = Boolean(matchPath({ path: to, end: to === '/' }, pathname));
            return (
              <OverflowTooltip key={to} content={t(label)}>
                <NavLink
                  to={to}
                  end={to === '/'}
                  className={cn(getNavLinkClass({ isActive: active }), !showLabels && !active && 'gap-0')}
                  aria-label={t(label)}
                  data-tauri-drag-region="false"
                  onClick={(event) => guardNavigation(event, to)}
                >
                  <Icon className="size-4.5 shrink-0" aria-hidden="true" />
                  <span className={cn('truncate', !showLabels && !active && 'w-0')} aria-hidden={!showLabels && !active}>
                    <span data-nav-label className="block w-max whitespace-nowrap">{t(label)}</span>
                  </span>
                </NavLink>
              </OverflowTooltip>
            );
          })}
        </nav>
      </div>

      {/* Right: Tool Buttons & Window Controls */}
      <div
        className={cn(
          'flex shrink-0 items-center gap-0.5 lg:gap-1 min-w-0 justify-end h-full',
          windows ? 'pr-0' : 'pr-2 sm:pr-3',
        )}
        data-tauri-drag-region
      >
        <InstallWizardStatusControl />
        <RecoveryCenter />
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="cursor-pointer text-xs font-bold font-mono text-zinc-600 dark:text-muted-foreground hover:text-foreground transition-colors shrink-0 h-7 w-7 sm:h-7.5 sm:w-7.5"
            >
              {LOCALE_OPTIONS.find((o) => o.value === locale)?.code}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {LOCALE_OPTIONS.map((option) => (
              <DropdownMenuItem
                key={option.value}
                onClick={() => setLocale(option.value)}
                className="cursor-pointer text-xs"
              >
                <span className="font-mono text-xs w-6">{option.code}</span>
                <span>{option.label}</span>
                {locale === option.value && (
                  <Check className="h-3.5 w-3.5 ml-auto text-primary" />
                )}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {/* Theme Toggle */}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="cursor-pointer text-muted-foreground hover:text-foreground shrink-0 h-7 w-7 sm:h-7.5 sm:w-7.5"
              onClick={toggleTheme}
              aria-label={t(theme === 'light' ? 'theme.switchToDark' : 'theme.switchToLight')}
            >
              {theme === 'light' ? <Sun className="h-3.5 w-3.5" /> : <Moon className="h-3.5 w-3.5" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t(theme === 'light' ? 'theme.switchToDark' : 'theme.switchToLight')}</TooltipContent>
        </Tooltip>

        {/* Window Controls (Windows only: macOS uses native traffic lights, Linux uses native window frame) */}
        {windows && <WindowControls className="ml-1" />}
      </div>
    </header>
  );
}
