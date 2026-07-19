import { useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { events } from '@/bindings';
import { useOptionalUnsavedChanges } from '@/lifecycle/unsaved-changes-context';

export function AgentConfigurationRequestRouter() {
  const navigate = useNavigate();
  const unsavedChanges = useOptionalUnsavedChanges();
  const opening = useRef(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const openAgent = async (agentId: string) => {
      if (disposed || opening.current) return;
      opening.current = true;
      const params = new URLSearchParams({
        section: 'agents',
        view: 'new',
        configureAgent: agentId,
      });
      const target = `/settings?${params.toString()}`;
      try {
        if (unsavedChanges) await unsavedChanges.guard(() => navigate(target));
        else navigate(target);
      } finally {
        opening.current = false;
      }
    };

    void events.agentConfigurationRequestedEvent.listen((event) => {
      void openAgent(event.payload.agentId);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate, unsavedChanges]);

  return null;
}
