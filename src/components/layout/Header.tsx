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
    'h-full flex items-center gap-1.5 px-1 font-heading font-semibold text-sm tracking-tight transition-colors',
    isActive
      ? 'text-primary border-b-2 border-primary font-bold'
      : 'text-muted-foreground hover:text-primary border-b-2 border-transparent'
  );

const LOCALE_OPTIONS: { value: Locale; code: string; label: string }[] = [
  { value: 'zh-CN', code: 'ZH', label: '简体中文' },
  { value: 'en', code: 'EN', label: 'English' },
];

export function Header() {
  const { t } = useTranslation();
  const { theme, toggleTheme, locale, setLocale } = useSettingsStore();

  return (
    <header className="flex h-14 items-center justify-between px-4 sm:px-6 border-b border-border bg-background/95 backdrop-blur flex-shrink-0">
      {/* Left: Logo + Brand + Nav */}
      <div className="flex h-full items-center gap-6">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center bg-primary">
            <span className="text-base font-bold text-primary-foreground">S</span>
          </div>
          <span className="hidden sm:inline font-heading text-xl font-extrabold text-primary tracking-tighter">
            {t('app.name')}
          </span>
        </div>

        <nav className="flex h-full items-center gap-4">
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
      </div>

      {/* Right: Tool Buttons */}
      <div className="flex items-center gap-1">
        {/* Language Selector */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="sm"
              className="cursor-pointer text-sm font-semibold text-muted-foreground hover:text-foreground"
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
