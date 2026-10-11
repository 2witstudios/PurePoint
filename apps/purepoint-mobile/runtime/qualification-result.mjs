/** Required capabilities observed in the pinned published candidates. */
export const requirements = {
  "pi-web-access": {
    tools: [
      "web_search",
      "source_check",
      "fetch_content",
      "get_search_content",
    ],
  },
  "pi-playwright": { skills: ["playwright-browser"] },
  "@amaster.ai/pi-computer-use": {
    tools: [
      "computer_use_tools",
      "computer_use_list_apps",
      "computer_use_get_window_state",
      "computer_use_verify_state",
      "computer_use_launch_app",
      "computer_use_click",
      "computer_use_double_click",
      "computer_use_right_click",
      "computer_use_drag",
      "computer_use_type_text",
      "computer_use_press_key",
      "computer_use_hotkey",
      "computer_use_set_value",
      "computer_use_scroll",
      "computer_use_zoom",
    ],
  },
};

/** A working fixture command alone does not prove the candidate loaded. */
export function catalogResult(record) {
  if (record.extensionErrors.length) return "FAIL extension lifecycle";
  const required = requirements[record.name];
  if (!required) return "FAIL unknown candidate capability requirements";
  const tools = new Set(record.catalog.tools.map((tool) => tool.name));
  const skills = new Set(record.catalog.skills);
  const missing = [
    ...(required.tools ?? []).filter((name) => !tools.has(name)),
    ...(required.skills ?? []).filter((name) => !skills.has(name)),
  ];
  return missing.length
    ? "FAIL missing candidate capabilities: " + missing.join(", ")
    : "PASS native RPC startup, catalog and command output; real integration actions NOT RUN";
}

/** Confirm exit after Rpc.close's SIGTERM fallback, without an unbounded wait. */
export async function confirmExit(child, timeoutMs = 2000) {
  if (child.exitCode !== null || child.signalCode !== null) return true;
  return new Promise((resolve) => {
    const finish = (exited) => {
      clearTimeout(timer);
      child.removeListener("exit", onExit);
      resolve(exited);
    };
    const onExit = () => finish(true);
    const timer = setTimeout(() => finish(false), timeoutMs);
    child.once("exit", onExit);
  });
}

/** Intentional engine rejection is diagnostic, while failed probes fail the run. */
export function qualificationFailed(records) {
  return records.some(
    (record) => record.result.startsWith("FAIL") || record.teardown === false,
  );
}
