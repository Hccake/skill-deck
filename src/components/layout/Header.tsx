import { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink, useNavigate } from 'react-router-dom';
import { Sun, Moon, Package, Settings, Check, Compass, LibraryBig } from 'lucide-react';
import { Button } from '@/components/ui/button';
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
    'group flex h-8 shrink-0 items-center gap-1.5 px-2.5 sm:px-3 rounded-md text-sm leading-5 font-medium tracking-normal transition-colors duration-150 select-none border',
    isActive
      ? 'bg-background text-primary font-semibold shadow-xs border-border/60'
      : 'border-transparent text-zinc-600 dark:text-muted-foreground hover:text-foreground hover:bg-background/50'
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
    if (!space || !nav) return;
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
      setShowLabels((previous) => fullWidth + (previous ? 0 : 8) <= available);
    };
    const observer = new ResizeObserver(measure);
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
      className="flex h-12 items-center border-b border-border/80 bg-background/95 backdrop-blur-md shrink-0 gap-2 overflow-hidden select-none"
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

      {/* Center: Desktop Segmented Navigation */}
      <div ref={navigationSpace} className="flex h-full min-w-0 flex-1 items-center justify-center" data-tauri-drag-region>
      <nav
        ref={navigation}
        className="flex w-max shrink-0 items-center gap-0.5 p-0.5 sm:p-1 bg-muted/60 dark:bg-muted/40 rounded-lg border border-border/50"
        data-tauri-drag-region="false"
      >
        {NAV_ITEMS.map(({ to, label, icon: Icon }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) => cn(getNavLinkClass({ isActive }), !showLabels && !isActive && 'gap-0')}
            aria-label={t(label)}
            title={t(label)}
            onClick={(event) => guardNavigation(event, to)}
          >
            {({ isActive }) => <>
              <Icon className="size-4.5 shrink-0" aria-hidden="true" />
              <span className={cn('overflow-hidden', !showLabels && !isActive && 'w-0')} aria-hidden={!showLabels && !isActive}>
                <span data-nav-label className="block w-max whitespace-nowrap">{t(label)}</span>
              </span>
            </>}
          </NavLink>
        ))}
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
        <Button
          variant="ghost"
          size="icon"
          className="cursor-pointer text-muted-foreground hover:text-foreground shrink-0 h-7 w-7 sm:h-7.5 sm:w-7.5"
          onClick={toggleTheme}
          aria-label={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
          title={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
        >
          {theme === 'light' ? (
            <Sun className="h-3.5 w-3.5" />
          ) : (
            <Moon className="h-3.5 w-3.5" />
          )}
        </Button>

        {/* Window Controls (Windows only: macOS uses native traffic lights, Linux uses native window frame) */}
        {windows && <WindowControls className="ml-1" />}
      </div>
    </header>
  );
}
