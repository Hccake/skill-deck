import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { OverflowTooltip } from '../overflow-tooltip';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../tooltip';

afterEach(() => {
  vi.useRealTimers();
});

it('keeps the hover delay after passing over text that needs no tooltip', () => {
  vi.useFakeTimers();
  render(<TooltipProvider>
    <OverflowTooltip content="Short"><button>Short</button></OverflowTooltip>
    <Tooltip><TooltipTrigger asChild><button>Action</button></TooltipTrigger><TooltipContent>Action help</TooltipContent></Tooltip>
  </TooltipProvider>);
  const short = screen.getByRole('button', { name: 'Short' });
  fireEvent.pointerMove(short, { pointerType: 'mouse' });
  act(() => vi.advanceTimersByTime(500));
  expect(screen.queryByRole('tooltip')).toBeNull();
  fireEvent.pointerLeave(short);
  fireEvent.pointerMove(screen.getByRole('button', { name: 'Action' }), { pointerType: 'mouse' });
  act(() => vi.advanceTimersByTime(399));
  expect(screen.queryByRole('tooltip')).toBeNull();
  act(() => vi.advanceTimersByTime(1));
  expect(screen.getByRole('tooltip').textContent).toBe('Action help');
});
