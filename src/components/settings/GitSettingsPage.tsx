import { useTranslation } from 'react-i18next';
import { GitCloneTimeoutSection } from './GitCloneTimeoutSection';
import { GithubCredentialSection } from './GithubCredentialSection';

export function GitSettingsPage() {
  const { t } = useTranslation();

  return (
    <div className="space-y-5">
      <header className="space-y-1">
        <h2 className="text-lg font-semibold tracking-tight text-foreground">
          {t('settings.git.title')}
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">
          {t('settings.git.description')}
        </p>
      </header>

      <div className="max-w-3xl overflow-hidden rounded-md border border-border/70 bg-background">
        <GithubCredentialSection />
        <div className="border-t border-border/70">
          <GitCloneTimeoutSection />
        </div>
      </div>
    </div>
  );
}
