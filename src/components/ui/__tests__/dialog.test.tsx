/* @vitest-environment jsdom */
import '@/test-utils';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from '../dialog';
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
} from '../alert-dialog';

describe('business dialog dismissal contract', () => {
  it('blocks the overlay while keeping Escape and the localized close button available', () => {
    const onOpenChange = vi.fn();
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent closeLabel="关闭">
          <DialogTitle>业务操作</DialogTitle>
          <DialogDescription>操作说明</DialogDescription>
        </DialogContent>
      </Dialog>,
    );

    fireEvent.pointerDown(document.querySelector('[data-slot="dialog-overlay"]')!);
    expect(onOpenChange).not.toHaveBeenCalled();

    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onOpenChange).toHaveBeenCalledWith(false);

    onOpenChange.mockClear();
    fireEvent.click(screen.getByRole('button', { name: '关闭' }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it('blocks Escape and hides the close button when dismissal is disabled', () => {
    const onOpenChange = vi.fn();
    render(
      <Dialog open onOpenChange={onOpenChange}>
        <DialogContent dismissible={false} closeLabel="关闭">
          <DialogTitle>正在执行</DialogTitle>
          <DialogDescription>请等待操作完成</DialogDescription>
        </DialogContent>
      </Dialog>,
    );

    fireEvent.keyDown(document, { key: 'Escape' });

    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: '关闭' })).toBeNull();
  });

  it('blocks AlertDialog Escape while an operation is executing', () => {
    const onOpenChange = vi.fn();
    render(
      <AlertDialog open onOpenChange={onOpenChange}>
        <AlertDialogContent dismissible={false}>
          <AlertDialogTitle>正在删除</AlertDialogTitle>
          <AlertDialogDescription>请等待操作完成</AlertDialogDescription>
        </AlertDialogContent>
      </AlertDialog>,
    );

    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.pointerDown(document.querySelector('[data-slot="alert-dialog-overlay"]')!);

    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
