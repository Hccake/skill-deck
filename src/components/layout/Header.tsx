import { useTranslation } from 'react-i18next';
import { NavLink, useNavigate } from 'react-router-dom';
import { Sun, Moon, Package, Settings, Check, Compass } from 'lucide-react';
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
import type { Locale } from '@/stores/settings';
import logoUrl from '@/assets/logo.png';
import { useOptionalUnsavedChanges } from '@/lifecycle/unsaved-changes-context';

// Hoisted outside component to avoid recreation on each render
const getNavLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'flex items-center gap-1.5 px-3.5 py-1.5 rounded-full text-sm font-medium transition-all duration-200',
    isActive
      ? 'bg-foreground text-background shadow-sm'
      : 'text-muted-foreground hover:text-foreground hover:bg-foreground/10 dark:hover:bg-foreground/15'
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

  return (
    <header className="flex h-14 items-center justify-between px-3 sm:px-6 border-b border-border bg-background/95 backdrop-blur flex-shrink-0 gap-2 sm:gap-4 overflow-hidden">
      {/* Left: Logo + Brand */}
      <div className="flex items-center gap-2 sm:gap-2.5 flex-1 min-w-0">
        <div className="flex items-center justify-center transition-transform hover:scale-105 shrink-0">
          <img src={logoUrl} alt="Logo" className="h-7 w-7 sm:h-8 sm:w-8 object-contain" />
        </div>
        <span className="hidden sm:inline font-heading text-lg font-bold text-primary tracking-tight truncate">
          {t('app.name')}
        </span>
      </div>

      {/* Center: Segmented Navigation (Capsule Shape for Global Nav) */}
      <nav className="flex items-center space-x-0.5 sm:space-x-1 bg-muted/40 p-1 rounded-full border border-border/50 shrink-0">
        <NavLink to="/" end className={getNavLinkClass} onClick={(event) => guardNavigation(event, '/')}>
          <Package className="h-3.5 w-3.5 sm:h-4 sm:w-4 shrink-0" />
          <span className="hidden min-[400px]:inline">{t('nav.skills')}</span>
        </NavLink>
        <NavLink to="/discover" className={getNavLinkClass} onClick={(event) => guardNavigation(event, '/discover')}>
          <Compass className="h-3.5 w-3.5 sm:h-4 sm:w-4 shrink-0" />
          <span className="hidden min-[400px]:inline">{t('nav.discover')}</span>
        </NavLink>
        <NavLink to="/settings" className={getNavLinkClass} onClick={(event) => guardNavigation(event, '/settings')}>
          <Settings className="h-3.5 w-3.5 sm:h-4 sm:w-4 shrink-0" />
          <span className="hidden min-[400px]:inline">{t('nav.settings')}</span>
        </NavLink>
      </nav>

      {/* Right: Tool Buttons */}
      <div className="flex items-center gap-0.5 sm:gap-1 flex-1 min-w-0 justify-end">
        <RecoveryCenter />
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="cursor-pointer text-sm font-bold font-mono text-muted-foreground hover:text-foreground transition-colors shrink-0 h-8 w-8 sm:h-9 sm:w-9"
            >
              {LOCALE_OPTIONS.find((o) => o.value === locale)?.code}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {LOCALE_OPTIONS.map((option) => (
              <DropdownMenuItem
                key={option.value}
                onClick={() => setLocale(option.value)}
                className="cursor-pointer"
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
          className="cursor-pointer text-muted-foreground hover:text-foreground shrink-0 h-8 w-8 sm:h-9 sm:w-9"
          onClick={toggleTheme}
          aria-label={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
          title={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
        >
          {theme === 'light' ? (
            <Sun className="h-4 w-4 sm:h-5 sm:w-5" />
          ) : (
            <Moon className="h-4 w-4 sm:h-5 sm:w-5" />
          )}
        </Button>
      </div>
    </header>
  );
}
