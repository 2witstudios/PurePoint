import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';
import './theme/tokens.css';
import './styles.css';
import { AppState } from './state/appState.ts';
import { startApp } from './views/shell/bootstrap.ts';

const app = new AppState();
startApp(app);

const root = document.getElementById('root');
if (!root) {
  throw new Error('PurePoint: #root element missing from index.html');
}

createRoot(root).render(
  <StrictMode>
    <App app={app} />
  </StrictMode>,
);
