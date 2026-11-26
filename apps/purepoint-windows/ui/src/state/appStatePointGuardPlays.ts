// Port of apps/purepoint-macos/purepoint-macos/State/AppState+PointGuardPlays.swift (WIN-8.13).
//
// Point Guard's plays read only global `pu` prompts, so they're listed from this PC's home
// folder: its `.pu\templates` is the global folder itself, and a remote daemon has no such
// folder, so no project's local prompt can shadow a global one of the same name.
//
// Differences from Swift: these are functions over the app's Agents hub state
// (`agentsHubStateFor(app)`) rather than an AppState extension, and the home folder comes from
// Tauri's path API (the Swift `NSHomeDirectory()`).
import { homeDir } from '@tauri-apps/api/path';
import type { SavedPrompt } from '../model/agentsHubModels.ts';
import type { PointGuardPrompt } from '../model/pointGuardPlays.ts';
import { agentsHubStateFor } from './agentsHubState.ts';

/** What the plays need from the Agents hub state. */
export interface PointGuardPromptSource {
  listTemplates(root: string): Promise<SavedPrompt[] | null>;
  template(root: string, name: string): Promise<SavedPrompt | null>;
}

let home: Promise<string> | null = null;

/** The home folder, read once. */
export function pointGuardPromptRoot(): Promise<string> {
  home ??= homeDir();
  return home;
}

const asPrompt = (prompt: SavedPrompt): PointGuardPrompt => ({
  name: prompt.name,
  body: prompt.body,
  needsInput: prompt.variables.length > 0,
});

/** The global prompts the court can play, freshly listed; null when the daemon can't be reached. */
export async function pointGuardPrompts(
  source: PointGuardPromptSource,
  root: Promise<string> = pointGuardPromptRoot(),
): Promise<PointGuardPrompt[] | null> {
  let folder: string;
  try {
    folder = await root;
  } catch {
    return null;
  }
  const templates = await source.listTemplates(folder);
  return templates === null ? null : templates.map(asPrompt);
}

/** A saved prompt as it is now, body included; null when it no longer exists. */
export async function pointGuardPrompt(
  source: PointGuardPromptSource,
  name: string,
  root: Promise<string> = pointGuardPromptRoot(),
): Promise<PointGuardPrompt | null> {
  let folder: string;
  try {
    folder = await root;
  } catch {
    return null;
  }
  const prompt = await source.template(folder, name);
  return prompt === null ? null : asPrompt(prompt);
}

/** The app's prompt source: its Agents hub state. */
export function promptSourceFor(app: object): PointGuardPromptSource {
  return agentsHubStateFor(app);
}
