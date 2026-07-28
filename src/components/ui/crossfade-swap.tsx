import { Component, type ReactNode } from 'react';
import { cn } from '@/lib/utils';

export const CROSSFADE_DURATION_MS = 160;

interface CrossfadeSwapProps {
  transitionKey: string;
  children: ReactNode;
  className?: string;
}

interface CrossfadeSwapState {
  transitionKey: string;
  current: ReactNode;
  outgoing: ReactNode | null;
  transitionId: number;
}

export class CrossfadeSwap extends Component<CrossfadeSwapProps, CrossfadeSwapState> {
  state: CrossfadeSwapState = {
    transitionKey: this.props.transitionKey,
    current: this.props.children,
    outgoing: null,
    transitionId: 0,
  };

  private clearOutgoingTimer: ReturnType<typeof setTimeout> | null = null;

  static getDerivedStateFromProps(
    nextProps: CrossfadeSwapProps,
    previousState: CrossfadeSwapState,
  ): Partial<CrossfadeSwapState> {
    if (nextProps.transitionKey === previousState.transitionKey) {
      return { current: nextProps.children };
    }
    return {
      transitionKey: nextProps.transitionKey,
      current: nextProps.children,
      outgoing: previousState.current,
      transitionId: previousState.transitionId + 1,
    };
  }

  componentDidUpdate(
    _previousProps: CrossfadeSwapProps,
    previousState: CrossfadeSwapState,
  ) {
    if (this.state.transitionId === previousState.transitionId) return;
    if (this.clearOutgoingTimer) clearTimeout(this.clearOutgoingTimer);
    const transitionId = this.state.transitionId;
    this.clearOutgoingTimer = setTimeout(() => {
      this.clearOutgoingTimer = null;
      this.setState((state) => (
        state.transitionId === transitionId ? { outgoing: null } : null
      ));
    }, CROSSFADE_DURATION_MS);
  }

  componentWillUnmount() {
    if (this.clearOutgoingTimer) clearTimeout(this.clearOutgoingTimer);
  }

  render() {
    const { className } = this.props;
    const { current, outgoing, transitionId } = this.state;
    if (current == null && outgoing == null) return null;

    return (
      <span className={cn('inline-grid align-middle', className)}>
        {outgoing != null ? (
          <span
            key={`outgoing:${transitionId}`}
            data-crossfade-state="outgoing"
            aria-hidden="true"
            inert
            className="pointer-events-none col-start-1 row-start-1 animate-out fade-out duration-[160ms] motion-reduce:hidden"
          >
            {outgoing}
          </span>
        ) : null}
        {current != null ? (
          <span
            key={`current:${transitionId}`}
            data-crossfade-state="current"
            className="col-start-1 row-start-1 animate-in fade-in duration-[160ms] motion-reduce:animate-none"
          >
            {current}
          </span>
        ) : null}
      </span>
    );
  }
}
