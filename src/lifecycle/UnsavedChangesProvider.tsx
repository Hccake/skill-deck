import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { useBlocker } from 'react-router-dom';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  UnsavedChangesContext,
  type UnsavedChangesRegistration,
} from './unsaved-changes-context';

interface GuardedAction {
  action: () => void | Promise<void>;
  resolve: (performed: boolean) => void;
}

export function UnsavedChangesProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const registrationRef = useRef<UnsavedChangesRegistration | null>(null);
  const queueRef = useRef<GuardedAction[]>([]);
  const activeRef = useRef<GuardedAction | null>(null);
  const pumpRef = useRef<() => void>(() => undefined);
  const bypassNavigationRef = useRef(false);
  const handledBlockerRef = useRef<string | null>(null);
  const [open, setOpen] = useState(false);

  const register = useCallback((registration: UnsavedChangesRegistration) => {
    registrationRef.current = registration;
    return () => {
      if (registrationRef.current === registration) registrationRef.current = null;
    };
  }, []);

  const pump = useCallback(() => {
    if (activeRef.current) return;
    const next = queueRef.current.shift();
    if (!next) return;
    if (!registrationRef.current?.dirty) {
      activeRef.current = next;
      void Promise.resolve()
        .then(next.action)
        .then(
          () => next.resolve(true),
          () => next.resolve(false),
        )
        .finally(() => {
          activeRef.current = null;
          pumpRef.current();
        });
      return;
    }
    activeRef.current = next;
    setOpen(true);
  }, []);

  useEffect(() => {
    pumpRef.current = pump;
  }, [pump]);

  const guard = useCallback((action: () => void | Promise<void>) => (
    new Promise<boolean>((resolve) => {
      if (!registrationRef.current?.dirty && !activeRef.current && queueRef.current.length === 0) {
        void Promise.resolve()
          .then(action)
          .then(
            () => resolve(true),
            () => resolve(false),
          );
        return;
      }
      queueRef.current.push({ action, resolve });
      pump();
    })
  ), [pump]);

  const finish = useCallback(async (discard: boolean) => {
    const current = activeRef.current;
    if (!current) return;
    setOpen(false);
    if (!discard) {
      activeRef.current = null;
      current.resolve(false);
      pump();
      return;
    }
    bypassNavigationRef.current = true;
    let performed = false;
    try {
      await registrationRef.current?.discard();
      await current.action();
      performed = true;
    } catch {
      performed = false;
    } finally {
      bypassNavigationRef.current = false;
      activeRef.current = null;
      current.resolve(performed);
      pump();
    }
  }, [pump]);

  const shouldBlockNavigation = useCallback(() => (
    Boolean(registrationRef.current?.dirty) && !bypassNavigationRef.current
  ), []);
  const blocker = useBlocker(shouldBlockNavigation);

  useEffect(() => {
    if (blocker.state !== 'blocked') {
      handledBlockerRef.current = null;
      return;
    }
    const blockedLocationKey = blocker.location.key;
    if (handledBlockerRef.current === blockedLocationKey) return;
    handledBlockerRef.current = blockedLocationKey;

    void guard(() => blocker.proceed()).then((performed) => {
      if (!performed && blocker.state === 'blocked') blocker.reset();
    });
  }, [blocker, guard]);

  const value = useMemo(() => ({ register, guard }), [guard, register]);

  return (
    <UnsavedChangesContext.Provider value={value}>
      {children}
      <AlertDialog open={open}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('settings.agents.dirtyNavigation.title')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('settings.agents.dirtyNavigation.description')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => void finish(false)}>
              {t('settings.agents.dirtyNavigation.stay')}
            </AlertDialogCancel>
            <AlertDialogAction onClick={() => void finish(true)}>
              {t('settings.agents.dirtyNavigation.discard')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </UnsavedChangesContext.Provider>
  );
}
