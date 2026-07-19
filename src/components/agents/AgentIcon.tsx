import { Bot } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { AgentId } from '@/bindings';

interface AgentIconProps {
  agentId?: AgentId;
  className?: string;
  iconClassName?: string;
}

export function AgentIcon({ className, iconClassName }: AgentIconProps) {
  return (
    <div
      className={cn(
        'flex shrink-0 items-center justify-center rounded-md border border-border/50 bg-muted/60',
        className,
      )}
    >
      <Bot className={cn('h-4 w-4 text-foreground/70', iconClassName)} />
    </div>
  );
}
