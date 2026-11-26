// Port of `shellEscape` from apps/purepoint-macos/purepoint-macos/Services/ShellUtilities.swift
// (WIN-6.0). Its other helper, `parseJSONLine`, is `parse_json_line` in
// `src-tauri/src/conversations/util.rs`, where the transcript parsers that call it live.

/**
 * POSIX shell quoting: wrap in single quotes and escape embedded single quotes. The terminal uses
 * it for paths dropped onto an agent pane (the agent shells, local or remote, are POSIX-style).
 */
export function shellEscape(value: string): string {
  return "'" + value.replaceAll("'", "'\\''") + "'";
}
