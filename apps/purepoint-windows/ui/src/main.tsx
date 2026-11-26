import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import './theme/tokens.css';
import './styles.css';
import { startThemeSync } from './theme/themeStore.ts';

startThemeSync();

const root = document.getElementById('root');
if (!root) {
  throw new Error('PurePoint: #root element missing from index.html');
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
