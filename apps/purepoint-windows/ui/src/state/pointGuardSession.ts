// The app's one Point Guard: its chat (`PiChatModel`), the owned runtime's setup state
// (`PointGuardServiceState`) and the court's playbook, as the Swift AppState holds
// `pointGuardChat`, `pointGuardService` and `PointGuardPlaybook.shared` (WIN-8.13).
//
// `startPointGuardSession` is what purepoint_macosApp.swift and ContentView start for Point Guard:
// the service follows the backend (which launches the runtime itself, WIN-9.3) and binds the chat
// once the runtime is ready; the runtime's working folder becomes `AppState.pointGuardRoot`
// (ContentView's `onReceive(pointGuardService.$activeCwd)`), activating the Point Guard workspace
// when the dashboard is showing.
import { createStore, type StoreApi } from 'zustand/vanilla';
import type { AppState } from './appState.ts';
import { PiChatModel, pointGuardChatBinding } from './piChat.ts';
import { PointGuardServiceState } from './pointGuardService.ts';
import { PointGuardPlaybook, sharedPlaybook } from '../model/pointGuardPlays.ts';
import { browserStorage } from './storage.ts';

/** `@AppStorage("PP_pointGuardShowConversations")`, shared by every Point Guard surface. */
export const SHOW_CONVERSATIONS_KEY = 'PP_pointGuardShowConversations';

export interface PointGuardSession {
  readonly chat: PiChatModel;
  readonly service: PointGuardServiceState;
  readonly playbook: PointGuardPlaybook;
  /** Whether the conversation rail is open. */
  readonly showConversations: StoreApi<{ shown: boolean }>;
  setShowConversations(shown: boolean): void;
}

export interface PointGuardSessionOptions {
  chat?: PiChatModel;
  service?: PointGuardServiceState;
  playbook?: PointGuardPlaybook;
  storage?: Pick<Storage, 'getItem' | 'setItem'> | null;
}

function readShown(storage: PointGuardSessionOptions['storage']): boolean {
  try {
    return storage?.getItem(SHOW_CONVERSATIONS_KEY) === 'true';
  } catch {
    return false;
  }
}

export function makePointGuardSession(options: PointGuardSessionOptions = {}): PointGuardSession {
  const chat = options.chat ?? new PiChatModel();
  const service = options.service ?? new PointGuardServiceState(pointGuardChatBinding(chat));
  const storage = options.storage === undefined ? browserStorage() : options.storage;
  const showConversations = createStore<{ shown: boolean }>()(() => ({
    shown: readShown(storage),
  }));
  return {
    chat,
    service,
    playbook: options.playbook ?? sharedPlaybook(),
    showConversations,
    setShowConversations(shown) {
      showConversations.setState({ shown });
      try {
        storage?.setItem(SHOW_CONVERSATIONS_KEY, String(shown));
      } catch {
        // Storage unavailable: the choice lasts for this session.
      }
    },
  };
}

const sessions = new WeakMap<object, PointGuardSession>();

/** The app's Point Guard session, made on first use. */
export function pointGuardSessionFor(app: object): PointGuardSession {
  let session = sessions.get(app);
  if (session === undefined) {
    session = makePointGuardSession();
    sessions.set(app, session);
  }
  return session;
}

/** Install a session for `app` (tests and previews). */
export function setPointGuardSession(app: object, session: PointGuardSession): void {
  sessions.set(app, session);
}

/**
 * Attach the service and follow the runtime's working folder into `app.pointGuardRoot`;
 * returns the stop.
 */
export function startPointGuardSession(
  app: AppState,
  session: PointGuardSession = pointGuardSessionFor(app),
): () => void {
  let stopped = false;
  const follow = (cwd: string) => {
    const root = cwd === '' ? null : cwd;
    if (root === app.pointGuardRoot) return;
    app.updatePointGuardRoot(cwd);
    const selection = app.activeSidebarSelection;
    if (selection?.kind === 'nav' && selection.item === 'dashboard') {
      app.activatePointGuardWorkspace();
    }
  };
  follow(session.service.store.getState().activeCwd);
  const unsubscribe = session.service.store.subscribe((state, previous) => {
    if (state.activeCwd !== previous.activeCwd) follow(state.activeCwd);
  });
  session.service.attach().catch((error: unknown) => {
    if (!stopped) session.service.error = error instanceof Error ? error.message : String(error);
  });
  return () => {
    stopped = true;
    unsubscribe();
    session.service.detach();
  };
}
