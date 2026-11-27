import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App.tsx';
import { makeTestApp } from './views/shell/testApp.ts';

describe('App', () => {
  it('renders the PurePoint shell', () => {
    render(<App app={makeTestApp()} />);
    expect(screen.getByRole('menubar')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
    expect(screen.getByTestId('point-guard-view')).toBeInTheDocument();
  });
});
