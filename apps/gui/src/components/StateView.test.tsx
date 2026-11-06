import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { LoadingState, MutationError } from './StateView';

afterEach(cleanup);

it('exposes an accessible loading status without showing empty or failed content', () => {
  render(<LoadingState />);
  expect(screen.getByRole('status')).toHaveAccessibleName('加载中...');
  expect(screen.getByRole('status')).toHaveAttribute('aria-busy', 'true');
  expect(screen.queryByRole('alert')).toBeNull();
});

it('only retries on user input and preserves the failed operation variables', () => {
  const variables = { id: 17, command: 'pause' };
  const mutate = vi.fn();
  const mutation = { error: new Error('offline'), variables, mutate, isPending: false };
  const view = render(<MutationError mutation={mutation} />);
  expect(mutate).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '重试' }));
  expect(mutate).toHaveBeenCalledExactlyOnceWith(variables);
  view.rerender(<MutationError mutation={mutation} disabled />);
  fireEvent.click(screen.getByRole('button'));
  expect(mutate).toHaveBeenCalledTimes(1);
  view.rerender(<MutationError mutation={{ ...mutation, error: null }} />);
  expect(screen.queryByRole('alert')).toBeNull();
});
