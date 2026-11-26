import { TitleBar } from './views/TitleBar.tsx';

/** Minimal shell; the real window content arrives with WIN-7.1. */
export function App() {
  return (
    <div className="app">
      <TitleBar />
      <main className="app-content">
        <h1>PurePoint</h1>
      </main>
    </div>
  );
}
