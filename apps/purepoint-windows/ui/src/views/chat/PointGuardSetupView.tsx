// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PointGuardSetupView.swift (WIN-9.5):
// Point Guard setup — the owned runtime's status with Start/Stop/Restart, the working folder,
// the provider sign-in (browser OAuth or API key, with the provider's prompts), the model, and
// phone pairing with each paired device's access. `PointGuardPhoneView` is the Connect phone
// sheet. The same view is the Settings › Point Guard pane and the chat's Setup sheet.
//
// Differences from Swift: the working folder is picked with the Tauri folder dialog
// (`pickFolder`); the provider sign-in link opens through the opener plugin; the pairing code is
// shown as text with Copy (no QR encoder ships with the app yet), and "this PC" replaces "this
// Mac".
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import type { PiChatModel } from '../../state/piChat.ts';
import {
  PHONE_ACCESS_LEVELS,
  field,
  levelTitle,
  levelTerminals,
  levelWorkspaceManagement,
  phoneAccessLevel,
  phoneAccessSummary,
  text,
  values,
  type PointGuardServiceState,
} from '../../state/pointGuardService.ts';
import type { JsonValue } from '../../services/pointguard.ts';
import { pickFolder } from '../../services/dialog.ts';
import { openExternal } from '../../services/opener.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ChatSheet } from './pointGuardParts.tsx';

const flag = (value: JsonValue): boolean => value === true;

export function PointGuardSetupView({
  service,
  chat,
  embedded = false,
}: {
  service: PointGuardServiceState;
  chat: PiChatModel;
  /** In Settings, whose Connection group already shows the status and Start/Stop. */
  embedded?: boolean;
}) {
  const pg = useStore(service.store);
  const busy = useStore(chat.store, (state) => state.snapshot?.busy === true);
  const [provider, setProvider] = useState(pg.selectedProvider);
  const [model, setModel] = useState(pg.selectedModel);
  const [workingDirectory, setWorkingDirectory] = useState('');
  const [showPhone, setShowPhone] = useState(false);
  const selected: JsonValue =
    pg.providers.find((item) => text(field(item, 'id')) === provider) ?? null;

  // The app starts the runtime at launch; a failed start waits for Start / Retry (never retried
  // automatically, REQ-WINAPP-086), so appearing only refreshes a ready one.
  useEffect(() => {
    if (service.store.getState().ready) void service.refresh();
  }, [service]);

  // A selection that followed the saved provider keeps following it (adjusted during render).
  const [saved, setSaved] = useState({ provider: pg.selectedProvider, model: pg.selectedModel });
  if (saved.provider !== pg.selectedProvider || saved.model !== pg.selectedModel) {
    setSaved({ provider: pg.selectedProvider, model: pg.selectedModel });
    if (provider === '' || provider === saved.provider) {
      setProvider(pg.selectedProvider);
      setModel(pg.selectedModel);
    } else if (provider === pg.selectedProvider) {
      setModel(pg.selectedModel);
    }
  }

  // A typed response belongs to one provider prompt while it is pending; a new prompt or a
  // finished sign-in clears it.
  const authPromptId = text(field(field(pg.auth, 'prompt'), 'id'));
  const authStatus = text(field(pg.auth, 'status'));
  const responseKey = (authPromptId ?? '') + '|' + (authStatus ?? '');
  const [typed, setTyped] = useState({ key: responseKey, value: '' });
  const response = typed.key === responseKey ? typed.value : '';
  const setResponse = (value: string) => {
    setTyped({ key: responseKey, value });
  };

  const chooseProvider = (value: string) => {
    setProvider(value);
    setModel(value === pg.selectedProvider ? pg.selectedModel : '');
    setResponse('');
    if (text(field(pg.auth, 'provider')) !== value && pg.auth !== null) void service.cancelLogin();
  };

  const devices = pg.devices.flatMap((device) => {
    const id = text(field(device, 'deviceId'));
    return id === null ? [] : [{ id, device }];
  });
  const models = values(field(selected, 'models'));

  return (
    <div className="pg-setup" data-testid="pg-setup">
      {!embedded && (
        <>
          <div className="pg-setup-row">
            <span className="pg-setup-phase">
              <Icon name={pg.ready ? 'checkmark.circle' : 'circle'} size={14} />
              <span data-testid="pg-setup-phase">{pg.phase}</span>
            </span>
            <span className="pg-spacer" />
            {pg.ready ? (
              <>
                <button
                  type="button"
                  className="pg-button"
                  disabled={busy}
                  onClick={() => void service.stop()}
                >
                  Stop Point Guard
                </button>
                <button
                  type="button"
                  className="pg-button"
                  disabled={busy}
                  onClick={() => void service.restart(chat.busy)}
                >
                  Restart Point Guard
                </button>
              </>
            ) : (
              <button type="button" className="pg-button" onClick={() => void service.start()}>
                Start / Retry
              </button>
            )}
          </div>
          <p className="pg-setup-caption">
            Point Guard is included with PurePoint and updated with the app. Your provider and
            conversations are saved on this PC.
          </p>
        </>
      )}
      {!embedded && pg.error !== null && (
        <p className="pg-setup-error" role="alert">
          {pg.error}
        </p>
      )}
      {pg.hasPendingAuthCleanup && (
        <button type="button" className="pg-button" onClick={() => void service.retryAuthCleanup()}>
          Retry pending login cancellations
        </button>
      )}
      {pg.hasPendingEnrollmentCleanup && (
        <button
          type="button"
          className="pg-button"
          onClick={() => void service.retryEnrollmentCleanup()}
        >
          Retry pending pairing revocations
        </button>
      )}
      {pg.restartRequired && (
        <div className="pg-setup-row">
          <span>Setup saved. Apply when Point Guard is idle.</span>
          <button
            type="button"
            className="pg-button"
            disabled={busy}
            onClick={() => void service.restart(chat.busy)}
          >
            Apply and restart
          </button>
        </div>
      )}

      <fieldset className="pg-setup-group">
        <legend>Working folder</legend>
        <div className="pg-setup-row">
          <span className="pg-setup-path" data-testid="pg-setup-folder">
            {workingDirectory === '' ? pg.cwd : workingDirectory}
          </span>
          <span className="pg-spacer" />
          <button
            type="button"
            className="pg-button"
            disabled={busy}
            onClick={() => {
              void (async () => {
                const folder = await pickFolder({ title: 'Choose Point Guard’s working folder' });
                if (folder === null) return;
                setWorkingDirectory(folder);
                await service.configure(folder);
              })();
            }}
          >
            Choose…
          </button>
        </div>
      </fieldset>

      {pg.ready && (
        <fieldset className="pg-setup-group">
          <legend>Provider</legend>
          <label className="pg-setup-field">
            <span>Provider</span>
            <select
              aria-label="Provider"
              value={provider}
              onChange={(event) => {
                chooseProvider(event.target.value);
              }}
            >
              <option value="">Choose provider</option>
              {pg.providers.map((item, index) => {
                const id = text(field(item, 'id')) ?? '';
                return (
                  <option key={`${id}-${String(index)}`} value={id}>
                    {text(field(item, 'name')) ?? (id === '' ? 'Provider' : id)}
                  </option>
                );
              })}
            </select>
          </label>
          <div className="pg-setup-row">
            {flag(field(selected, 'oauth')) && (
              <button
                type="button"
                className="pg-button"
                disabled={busy}
                onClick={() => {
                  setResponse('');
                  void service.login(provider, 'oauth');
                }}
              >
                Sign in with browser
              </button>
            )}
            {flag(field(selected, 'apiKey')) && (
              <button
                type="button"
                className="pg-button"
                disabled={busy}
                onClick={() => {
                  setResponse('');
                  void service.login(provider, 'api_key');
                }}
              >
                Use API key
              </button>
            )}
            {flag(field(selected, 'configured')) && (
              <span className="pg-setup-ok">
                <Icon name="checkmark.circle.fill" size={14} /> Credentials saved
              </span>
            )}
          </div>
          <LoginInteraction
            service={service}
            provider={provider}
            response={response}
            setResponse={setResponse}
          />
          {models.length > 0 && (
            <div className="pg-setup-row">
              <label className="pg-setup-field">
                <span>Model</span>
                <select
                  aria-label="Model"
                  value={model}
                  onChange={(event) => {
                    setModel(event.target.value);
                  }}
                >
                  <option value="">Choose model</option>
                  {models.map((item, index) => {
                    const id = text(field(item, 'id')) ?? '';
                    return (
                      <option key={`${id}-${String(index)}`} value={id}>
                        {text(field(item, 'name')) ?? (id === '' ? 'Model' : id)}
                      </option>
                    );
                  })}
                </select>
              </label>
              <button
                type="button"
                className="pg-button"
                disabled={model === '' || busy}
                onClick={() => void service.selectModel(provider, model)}
              >
                Use model
              </button>
            </div>
          )}
        </fieldset>
      )}

      {pg.ready && (
        <fieldset className="pg-setup-group">
          <legend>Phone</legend>
          {pg.remoteRecovery !== null && <p className="pg-setup-caption">{pg.remoteRecovery}</p>}
          <div className="pg-setup-row">
            <span className="pg-setup-caption">
              Pair once. Saved devices reconnect through Tailscale.
            </span>
            <span className="pg-spacer" />
            <button
              type="button"
              className="pg-button"
              onClick={() => {
                setShowPhone(true);
                void service.connectPhone();
              }}
            >
              Connect phone
            </button>
          </div>
          {devices.map(({ id, device }) => (
            <DeviceRow key={id} service={service} id={id} device={device} />
          ))}
        </fieldset>
      )}
      {showPhone && (
        <PointGuardPhoneView
          service={service}
          onDone={() => {
            setShowPhone(false);
          }}
        />
      )}
    </div>
  );
}

function DeviceRow({
  service,
  id,
  device,
}: {
  service: PointGuardServiceState;
  id: string;
  device: JsonValue;
}) {
  const updating = useStore(service.store, (state) => state.deviceUpdates.has(id));
  const name = text(field(device, 'name')) ?? 'Phone';
  const manages = field(device, 'workspaceManagement') === true;
  const level = phoneAccessLevel(text(field(device, 'terminals')) ?? 'off', manages);
  return (
    <div className="pg-device" data-testid={`pg-device-${id}`}>
      <div className="pg-setup-row">
        <span>{name}</span>
        <span className="pg-spacer" />
        <select
          aria-label={`Access for ${name}`}
          value={level}
          disabled={updating}
          onChange={(event) => {
            const next = PHONE_ACCESS_LEVELS.find((candidate) => candidate === event.target.value);
            if (next !== undefined) void service.setAccess(id, next);
          }}
        >
          {PHONE_ACCESS_LEVELS.map((candidate) => (
            <option key={candidate} value={candidate}>
              {levelTitle(candidate)}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="pg-button pg-button-destructive"
          disabled={updating}
          onClick={() => void service.revokeDevice(id)}
        >
          Revoke
        </button>
      </div>
      <p className="pg-setup-caption">
        {phoneAccessSummary(levelTerminals(level), levelWorkspaceManagement(level))}
      </p>
    </div>
  );
}

function LoginInteraction({
  service,
  provider,
  response,
  setResponse,
}: {
  service: PointGuardServiceState;
  provider: string;
  response: string;
  setResponse: (value: string) => void;
}) {
  const auth = useStore(service.store, (state) => state.auth);
  const restartRequired = useStore(service.store, (state) => state.restartRequired);
  const status = text(field(auth, 'status'));
  if (text(field(auth, 'provider')) !== provider || status === null) return null;
  const prompt = field(auth, 'prompt');
  const promptId = text(field(prompt, 'id'));
  const error = text(field(auth, 'error')) ?? text(field(field(auth, 'error'), 'message'));
  return (
    <div className="pg-login" data-testid="pg-login">
      {status === 'complete' ? (
        <>
          <span className="pg-setup-ok" role="status">
            <Icon name="checkmark.circle.fill" size={14} />{' '}
            {service.authMessage ?? 'Sign-in complete'}
          </span>
          <p className="pg-setup-caption">
            {restartRequired
              ? 'Choose Apply and restart when Point Guard is idle to use these credentials.'
              : 'Choose a model to start chatting.'}
          </p>
        </>
      ) : (
        <p className="pg-setup-caption">{service.authMessage ?? 'Sign-in status unavailable'}</p>
      )}
      {status === 'pending' &&
        values(field(auth, 'events')).map((event, index) => {
          const message = text(field(event, 'message'));
          const code = text(field(event, 'userCode'));
          const link = text(field(event, 'url')) ?? text(field(event, 'verificationUri'));
          const instructions = text(field(event, 'instructions'));
          return (
            <div key={index} className="pg-login-event">
              {message !== null && <p>{message}</p>}
              {code !== null && <p className="pg-login-code">Code: {code}</p>}
              {link?.startsWith('https://') === true && (
                <button
                  type="button"
                  className="pg-link-button"
                  onClick={() => {
                    openExternal(link);
                  }}
                >
                  Open provider sign-in
                </button>
              )}
              {instructions !== null && <p className="pg-setup-caption">{instructions}</p>}
            </div>
          );
        })}
      {status === 'pending' && promptId !== null && (
        <div className="pg-login-prompt">
          <p>{text(field(prompt, 'message')) ?? 'Provider response'}</p>
          {text(field(prompt, 'type')) === 'select' ? (
            values(field(prompt, 'options')).map((option, index) => {
              const id = text(field(option, 'id'));
              return (
                <button
                  key={`${id ?? ''}-${String(index)}`}
                  type="button"
                  className="pg-button"
                  onClick={() => {
                    if (id !== null) void service.respond(id);
                  }}
                >
                  {text(field(option, 'label')) ?? id ?? 'Select'}
                </button>
              );
            })
          ) : (
            <form
              key={promptId}
              className="pg-setup-row"
              onSubmit={(event) => {
                event.preventDefault();
                if (response === '') return;
                const value = response;
                setResponse('');
                void service.respond(value);
              }}
            >
              <input
                type="password"
                className="pg-text-field"
                aria-label={text(field(prompt, 'placeholder')) ?? 'Response'}
                placeholder={text(field(prompt, 'placeholder')) ?? 'Response'}
                autoComplete="off"
                value={response}
                onChange={(event) => {
                  setResponse(event.target.value);
                }}
              />
              <button type="submit" className="pg-button" disabled={response === ''}>
                Continue
              </button>
            </form>
          )}
        </div>
      )}
      {error !== null && <p className="pg-setup-error">{error}</p>}
      {status === 'pending' && (
        <button
          type="button"
          className="pg-button"
          onClick={() => {
            setResponse('');
            void service.cancelLogin();
          }}
        >
          Cancel login
        </button>
      )}
    </div>
  );
}

/** The Connect phone sheet: a one-time pairing code, refreshed until it is used or expires. */
export function PointGuardPhoneView({
  service,
  onDone,
}: {
  service: PointGuardServiceState;
  onDone: () => void;
}) {
  const pg = useStore(service.store);
  const payload = text(field(pg.enrollment, 'payload'));
  const expiresAt = field(pg.enrollment, 'expiresAt');
  const enrollmentId = text(field(pg.enrollment, 'enrollmentId'));
  const [now, setNow] = useState(() => Date.now());
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    const run = { cancelled: false };
    // Stops once the code is used or expires (or the sheet closes).
    void service.pollEnrollment(() => run.cancelled);
    return () => {
      run.cancelled = true;
    };
  }, [service, enrollmentId]);

  const close = () => {
    void (async () => {
      await service.closeEnrollment();
      await service.refresh();
    })();
    onDone();
  };
  const expired = typeof expiresAt !== 'number' || now >= expiresAt;

  return (
    <ChatSheet title="Connect your phone" onDone={close} width={460} testId="pg-phone-sheet">
      <div className="pg-phone">
        <p>
          Open the iPhone app → Connections → Pair with a code. Keep Tailscale connected on both
          devices.
        </p>
        {pg.enrollmentStatus === 'consumed' ? (
          <span className="pg-setup-ok">
            <Icon name="checkmark.circle.fill" size={14} /> Phone connected
          </span>
        ) : payload !== null ? (
          expired ? (
            <p>This code has expired. Generate a new code.</p>
          ) : (
            <>
              <textarea
                className="pg-phone-code"
                readOnly
                value={payload}
                aria-label="Pairing code"
              />
              <div className="pg-setup-row">
                <button
                  type="button"
                  className="pg-button"
                  onClick={() => {
                    void navigator.clipboard.writeText(payload).then(() => {
                      setCopied(true);
                    });
                  }}
                >
                  {copied ? 'Copied' : 'Copy code'}
                </button>
                <span className="pg-setup-caption">
                  One-time code · expires {new Date(expiresAt).toLocaleTimeString()}
                </span>
              </div>
            </>
          )
        ) : (
          <p>{pg.error ?? 'Preparing a one-time pairing code…'}</p>
        )}
        <p className="pg-setup-caption">
          This code enrolls one device. Revoke saved devices from Point Guard setup.
        </p>
        <div className="pg-setup-row">
          {pg.hasPendingEnrollmentCleanup && (
            <button
              type="button"
              className="pg-button"
              onClick={() => void service.retryEnrollmentCleanup()}
            >
              Retry revocation
            </button>
          )}
          <button
            type="button"
            className="pg-button"
            onClick={() => {
              void (async () => {
                await service.closeEnrollment();
                await service.connectPhone();
              })();
            }}
          >
            New code
          </button>
        </div>
      </div>
    </ChatSheet>
  );
}
