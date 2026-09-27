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
    'group flex items-center gap-1.5 px-2.5 sm:px-3 py-1 rounded-md text-[13px] font-medium transition-colors duration-150 select-none border',
    isActive
      ? 'bg-background text-primary font-semibold shadow-xs border-border/60'
      : 'border-transparent text-muted-foreground hover:text-foreground hover:bg-background/50'
  );

const LOCALE_OPTIONS: { value: Locale; code: string; label: string }[] = [
  { value: 'zh-CN', code: 'ZH', label: '简体中文' },
  { value: 'en', code: 'EN', label: 'English' },
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

  return (
    <header
      data-tauri-drag-region
      className="grid h-12 grid-cols-[minmax(0,1fr)_auto_minmax(max-content,1fr)] items-center border-b border-border/80 bg-background/95 backdrop-blur-md shrink-0 gap-2 overflow-hidden select-none"
    >
      {/* Left: Logo, Title & Environment */}
      <div
        className={cn(
          'flex items-center gap-2 min-w-0',
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
      <nav
        className="flex items-center gap-0.5 p-0.5 sm:p-1 bg-muted/60 dark:bg-muted/40 rounded-lg border border-border/50 shrink-0"
        data-tauri-drag-region="false"
      >
        <NavLink to="/" end className={getNavLinkClass} aria-label={t('nav.skills')} title={t('nav.skills')} onClick={(event) => guardNavigation(event, '/')}>
          <Package className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span className="hidden group-aria-[current=page]:inline lg:inline">{t('nav.skills')}</span>
        </NavLink>
        <NavLink to="/libraries" className={getNavLinkClass} aria-label={t('nav.libraries')} title={t('nav.libraries')} onClick={(event) => guardNavigation(event, '/libraries')}>
          <LibraryBig className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span className="hidden group-aria-[current=page]:inline lg:inline">{t('nav.libraries')}</span>
        </NavLink>
        <NavLink to="/discover" className={getNavLinkClass} aria-label={t('nav.discover')} title={t('nav.discover')} onClick={(event) => guardNavigation(event, '/discover')}>
          <Compass className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span className="hidden group-aria-[current=page]:inline lg:inline">{t('nav.discover')}</span>
        </NavLink>
        <NavLink to="/settings" className={getNavLinkClass} aria-label={t('nav.settings')} title={t('nav.settings')} onClick={(event) => guardNavigation(event, '/settings')}>
          <Settings className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span className="hidden group-aria-[current=page]:inline lg:inline">{t('nav.settings')}</span>
        </NavLink>
      </nav>

      {/* Right: Tool Buttons & Window Controls */}
      <div
        className={cn(
          'flex items-center gap-0.5 lg:gap-1 min-w-0 justify-end h-full',
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
              className="cursor-pointer text-xs font-bold font-mono text-muted-foreground hover:text-foreground transition-colors shrink-0 h-7 w-7 sm:h-7.5 sm:w-7.5"
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
