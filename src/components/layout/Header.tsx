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

// Hoisted outside component to avoid recreation on each render
const getNavLinkClass = ({ isActive }: { isActive: boolean }) =>
  cn(
    'px-4 py-2 text-sm font-medium rounded-full transition-all duration-200',
    'flex items-center gap-1.5',
    isActive
      ? 'bg-foreground text-background shadow-sm'
      : 'text-muted-foreground hover:text-foreground'
  );

const LOCALE_OPTIONS: { value: Locale; code: string; label: string }[] = [
  { value: 'zh-CN', code: 'ZH', label: '简体中文' },
  { value: 'en', code: 'EN', label: 'English' },
];

export function Header() {
  const { t } = useTranslation();
  const { theme, toggleTheme, locale, setLocale } = useSettingsStore();

  return (
    <header className="flex h-16 items-center justify-between px-4 sm:px-6 border-b border-border/40 bg-background/80 backdrop-blur-sm">
      {/* Left: Logo + Brand */}
      <div className="flex items-center gap-2.5 min-w-[120px]">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-teal-500 to-teal-600 shadow-md shadow-primary/20">
          <span className="text-base font-bold text-white">S</span>
        </div>
        <span className="hidden sm:inline text-xl font-medium text-foreground tracking-tight">
          {t('app.name')}
        </span>
      </div>

      {/* Center: Navigation Tabs */}
      <nav className="flex items-center gap-1 rounded-full bg-muted p-1">
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
      <div className="flex items-center gap-1 min-w-[120px] justify-end">
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className="group h-10 w-10 rounded-full cursor-pointer"
            >
              <span className="flex h-9 w-9 items-center justify-center rounded-full bg-muted text-sm font-semibold text-muted-foreground transition-colors group-hover:bg-muted-foreground/15 group-hover:text-foreground">
                {LOCALE_OPTIONS.find((o) => o.value === locale)?.code}
              </span>
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
          className="group h-10 w-10 rounded-full cursor-pointer"
          onClick={toggleTheme}
          aria-label={t(`theme.${theme}`)}
          title={t(`theme.${theme}`)}
        >
          <span className="flex h-9 w-9 items-center justify-center rounded-full bg-muted text-muted-foreground transition-colors group-hover:bg-muted-foreground/15 group-hover:text-foreground">
            {theme === 'light' ? (
              <Sun className="h-8 w-8" />
            ) : (
              <Moon className="h-8 w-8" />
            )}
          </span>
        </Button>
      </div>
    </header>
  );
}
