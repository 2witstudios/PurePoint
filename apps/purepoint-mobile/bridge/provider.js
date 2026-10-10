import { randomUUID } from "node:crypto";

/** Native-only, cancellable provider setup. Credentials never enter status snapshots. */
export class ProviderSetup {
  constructor({
    credentials,
    providers,
    models,
    now = Date.now,
    ttlMs = 300000,
    deviceId = () => "",
  }) {
    this.credentials = credentials;
    this.providers = providers;
    this.models = models;
    this.now = now;
    this.ttlMs = ttlMs;
    this.deviceId = deviceId;
    this.attempts = new Map();
    this.active = null;
  }
  async list() {
    const stored = await this.credentials.list();
    return {
      providers: this.providers.map((p) => ({
        id: p.id,
        name: p.name ?? p.id,
        oauth: !!p.auth.oauth,
        apiKey: !!p.auth.apiKey?.login,
        configured: stored.some((c) => c.providerId === p.id),
        models: this.models
          .filter((m) => m.provider === p.id)
          .map((m) => ({ id: m.id, name: m.name ?? m.id })),
      })),
    };
  }
  start(provider, type) {
    const p = this.providers.find((p) => p.id === provider);
    const method =
      type === "oauth"
        ? p?.auth.oauth
        : type === "api_key"
          ? p?.auth.apiKey
          : null;
    if (!method?.login)
      throw new Error("Provider does not support this login method.");
    this.cancelActive();
    const a = {
      attemptId: randomUUID(),
      provider,
      type,
      status: "pending",
      expiresAt: this.now() + this.ttlMs,
      events: [],
      prompt: undefined,
      error: undefined,
      restartRequired: false,
      controller: new AbortController(),
      pending: null,
      timer: null,
    };
    this.active = a;
    this.attempts.set(a.attemptId, a);
    if (this.attempts.size > 32)
      this.attempts.delete(this.attempts.keys().next().value);
    a.timer = setTimeout(() => this.end(a, "expired"), this.ttlMs);
    a.timer.unref();
    this.run(a, method).catch(() => {});
    return { attemptId: a.attemptId, expiresAt: a.expiresAt };
  }
  valid(a) {
    if (a.status === "pending" && this.now() >= a.expiresAt)
      this.end(a, "expired");
    return (
      a === this.active &&
      a.status === "pending" &&
      !a.controller.signal.aborted
    );
  }
  async run(a, method) {
    try {
      let before;
      await this.credentials.modify(
        a.provider,
        async (current) => {
          before = JSON.stringify(current);
          return undefined;
        },
        { signal: a.controller.signal },
      );
      if (!this.valid(a)) return;
      const credential = await method.login(
        {
          signal: a.controller.signal,
          prompt: (p) => this.prompt(a, p),
          notify: (e) => {
            if (this.valid(a)) a.events = [...a.events, e].slice(-32);
          },
        },
        { getDeviceId: this.deviceId, agentName: "PurePoint Point Guard" },
      );
      if (!this.valid(a)) return;
      if (credential?.type !== a.type || (a.type === "oauth" && (!Number.isFinite(credential.expires) || credential.expires <= this.now())))
        throw new Error("Provider returned invalid or expired credentials.");
      await this.credentials.modify(
        a.provider,
        async (current) => {
          // Check under the native store lock, including external native Pi changes.
          if (!this.valid(a) || JSON.stringify(current) !== before)
            throw new Error("Login superseded.");
          return credential;
        },
        { signal: a.controller.signal },
      );
      if (this.valid(a)) {
        a.status = "complete";
        a.restartRequired = true;
        clearTimeout(a.timer);
      }
    } catch {
      if (a.status === "pending") {
        a.status = "failed";
        a.error = "Provider login failed or was superseded. Try again.";
        clearTimeout(a.timer);
      }
    } finally {
      a.pending?.reject(new Error("Login ended."));
      a.pending = null;
      a.prompt = undefined;
    }
  }
  prompt(a, p) {
    if (!this.valid(a)) return Promise.reject(new Error("Login ended."));
    if (!["text", "secret", "select", "manual_code"].includes(p.type))
      return Promise.reject(new Error("Unsupported provider prompt."));
    return new Promise((resolve, reject) => {
      const id = randomUUID();
      const cleanup = () => {
        p.signal?.removeEventListener("abort", abort);
        a.controller.signal.removeEventListener("abort", abort);
        if (a.prompt?.id === id) {
          a.prompt = undefined;
          a.pending = null;
        }
      };
      const abort = () => {
        cleanup();
        reject(new Error("Prompt canceled."));
      };
      a.prompt = {
        id,
        type: p.type,
        message: p.message,
        placeholder: p.placeholder,
        options: p.options,
      };
      a.pending = {
        resolve: (value) => {
          cleanup();
          resolve(value);
        },
        reject: (error) => {
          cleanup();
          reject(error);
        },
      };
      p.signal?.addEventListener("abort", abort, { once: true });
      a.controller.signal.addEventListener("abort", abort, { once: true });
      if (p.signal?.aborted || a.controller.signal.aborted) abort();
    });
  }
  get(id) {
    const a = this.attempts.get(id);
    if (!a) throw new Error("Login attempt no longer exists.");
    return a;
  }
  status(id) {
    const a = this.get(id);
    this.valid(a);
    return {
      attemptId: a.attemptId,
      provider: a.provider,
      type: a.type,
      status: a.status,
      expiresAt: a.expiresAt,
      events: a.events,
      prompt: a.prompt,
      error: a.error,
      restartRequired: a.restartRequired,
    };
  }
  respond(id, promptId, value) {
    const a = this.get(id);
    if (!this.valid(a) || a.prompt?.id !== promptId || !a.pending)
      throw new Error("Provider prompt is stale.");
    if (typeof value !== "string" || value.length > 16384)
      throw new Error("Provider response is invalid.");
    if (
      a.prompt.type === "select" &&
      !a.prompt.options?.some((o) => o.id === value)
    )
      throw new Error("Select a supported provider option.");
    a.pending.resolve(value);
    return {};
  }
  end(a, status) {
    if (a.status !== "pending") return;
    a.status = status;
    clearTimeout(a.timer);
    a.controller.abort();
    a.prompt = undefined;
  }
  cancel(id) {
    this.end(this.get(id), "canceled");
    return {};
  }
  cancelActive() {
    if (this.active) this.end(this.active, "canceled");
  }
  close() {
    this.cancelActive();
  }
}
