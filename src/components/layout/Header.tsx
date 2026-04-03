import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';
import { Sun, Moon, Package, Settings, Check, Compass } from 'lucide-react';
import { Button } from '@/components/ui/button';
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
  const { theme, toggleTheme, locale, setLocale } = useSettingsStore();

  return (
    <header className="relative flex h-14 items-center justify-between px-4 sm:px-6 border-b border-border bg-background/95 backdrop-blur flex-shrink-0">
      {/* Left: Logo + Brand */}
      <div className="flex items-center gap-2 sm:gap-2.5 z-10 box-border">
        <div className="flex items-center justify-center transition-transform hover:scale-105">
          <img src={logoUrl} alt="Logo" className="h-7 w-7 sm:h-8 sm:w-8 object-contain" />
        </div>
        <span className="hidden sm:inline font-heading text-lg font-bold text-primary tracking-tight">
          {t('app.name')}
        </span>
      </div>

      {/* Center: Segmented Navigation (Capsule Shape for Global Nav) */}
      <nav className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 hidden md:flex items-center space-x-1 bg-muted/40 p-1 rounded-full border border-border/50">
        <NavLink to="/" end className={getNavLinkClass}>
          <Package className="h-4 w-4" />
          <span>{t('nav.skills')}</span>
        </NavLink>
        <NavLink to="/discover" className={getNavLinkClass}>
          <Compass className="h-4 w-4" />
          <span>{t('nav.discover')}</span>
        </NavLink>
        <NavLink to="/settings" className={getNavLinkClass}>
          <Settings className="h-4 w-4" />
          <span>{t('nav.settings')}</span>
        </NavLink>
      </nav>

      {/* Right: Tool Buttons */}
      <div className="flex items-center gap-1 z-10 box-border">
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="cursor-pointer text-sm font-bold font-mono text-muted-foreground hover:text-foreground transition-colors"
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
          className="cursor-pointer text-muted-foreground hover:text-foreground"
          onClick={toggleTheme}
          aria-label={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
          title={t(`theme.${theme === 'light' ? 'dark' : 'light'}`)}
        >
          {theme === 'light' ? (
            <Sun className="h-5 w-5" />
          ) : (
            <Moon className="h-5 w-5" />
          )}
        </Button>
      </div>
    </header>
  );
}
