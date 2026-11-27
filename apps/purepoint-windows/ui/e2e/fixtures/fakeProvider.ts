// The Point Guard spec's model provider (WIN-9.5): a loopback OpenAI-compatible chat completions
// server that streams a canned reply, so the managed runtime's Pi runs a real turn without any
// real AI provider. Pi finds it through `models.json` and `settings.json` in the temp HOME's
// `.pi\agent` (`writePiAgentConfig`). It is also Pi's HTTP proxy there, refusing every other
// destination, so nothing Pi starts reaches the network.
//
// A prompt containing "slow" streams for about 30 seconds (time to queue a message and Stop);
// any other streams a short reply in a second.
import { mkdirSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import { connect, type AddressInfo } from 'node:net';
import type { Duplex } from 'node:stream';
import { join } from 'node:path';

export const FAKE_PROVIDER = 'fixture';
export const FAKE_MODEL = 'fixture-model';
export const SHORT_REPLY = [
  'Fixture ',
  'reply: ',
  'streamed ',
  'from ',
  'the ',
  'loopback ',
  'provider.',
];

interface ChatRequest {
  messages?: { role: string; content: unknown }[];
}

function lastUserText(body: ChatRequest): string {
  const user = (body.messages ?? []).filter((message) => message.role === 'user').at(-1);
  if (user === undefined) return '';
  if (typeof user.content === 'string') return user.content;
  if (Array.isArray(user.content)) {
    return user.content
      .map((part: unknown) =>
        typeof part === 'object' && part !== null && 'text' in part ? String(part.text) : '',
      )
      .join('');
  }
  return '';
}

export class FakeProvider {
  private constructor(
    private readonly server: Server,
    readonly port: number,
    /** The user prompts the provider was asked to answer, in order. */
    readonly prompts: readonly string[],
  ) {}

  static async start(): Promise<FakeProvider> {
    const prompts: string[] = [];
    const server = createServer((request, response) => {
      void handle(request, response, prompts);
    });
    // Pi's proxy agent tunnels every request with CONNECT: a tunnel to this server is let
    // through (to itself); every other destination is refused.
    server.on('connect', (request: IncomingMessage, socket: Duplex, head: Buffer) => {
      const own = (server.address() as AddressInfo).port;
      const [host, port] = (request.url ?? '').split(':');
      if ((host !== '127.0.0.1' && host !== 'localhost') || Number(port) !== own) {
        socket.end('HTTP/1.1 403 Forbidden\r\n\r\n');
        return;
      }
      const upstream = connect(own, '127.0.0.1', () => {
        socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
        if (head.length > 0) upstream.write(head);
        upstream.pipe(socket);
        socket.pipe(upstream);
      });
      upstream.on('error', () => {
        socket.destroy();
      });
      socket.on('error', () => {
        upstream.destroy();
      });
    });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
    return new FakeProvider(server, (server.address() as AddressInfo).port, prompts);
  }

  get baseUrl(): string {
    return `http://127.0.0.1:${String(this.port)}/v1`;
  }

  async stop(): Promise<void> {
    this.server.closeAllConnections();
    await new Promise<void>((resolve) => {
      this.server.close(() => {
        resolve();
      });
    });
  }

  /** Point the temp HOME's Pi at this provider (and proxy). */
  writePiAgentConfig(home: string): void {
    const dir = join(home, '.pi', 'agent');
    mkdirSync(dir, { recursive: true });
    writeFileSync(
      join(dir, 'models.json'),
      JSON.stringify(
        {
          providers: {
            [FAKE_PROVIDER]: {
              baseUrl: this.baseUrl,
              api: 'openai-completions',
              apiKey: 'fixture-key',
              models: [
                {
                  id: FAKE_MODEL,
                  name: 'Fixture model',
                  input: ['text'],
                  reasoning: false,
                  contextWindow: 32000,
                  maxTokens: 4096,
                  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
                },
              ],
            },
          },
        },
        null,
        2,
      ),
    );
    writeFileSync(
      join(dir, 'settings.json'),
      JSON.stringify(
        {
          defaultProvider: FAKE_PROVIDER,
          defaultModel: FAKE_MODEL,
          enableInstallTelemetry: false,
          httpProxy: `http://127.0.0.1:${String(this.port)}`,
          retry: { enabled: false },
        },
        null,
        2,
      ),
    );
  }
}

async function readBody(request: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString('utf8');
}

function chunk(content: string | null, finish: string | null): string {
  return `data: ${JSON.stringify({
    id: 'chatcmpl-fixture',
    object: 'chat.completion.chunk',
    created: Math.floor(Date.now() / 1000),
    model: FAKE_MODEL,
    choices: [
      {
        index: 0,
        delta: content === null ? {} : { role: 'assistant', content },
        finish_reason: finish,
      },
    ],
  })}\n\n`;
}

async function handle(
  request: IncomingMessage,
  response: ServerResponse,
  prompts: string[],
): Promise<void> {
  // A proxied request carries an absolute URL; a direct one a path.
  const url = new URL(request.url ?? '/', 'http://127.0.0.1');
  const local = url.hostname === '127.0.0.1' || url.hostname === 'localhost';
  if (!local) {
    response.writeHead(403).end();
    return;
  }
  if (request.method === 'GET' && url.pathname.endsWith('/models')) {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ object: 'list', data: [{ id: FAKE_MODEL, object: 'model' }] }));
    return;
  }
  if (request.method !== 'POST' || !url.pathname.endsWith('/chat/completions')) {
    response.writeHead(404).end();
    return;
  }
  const body = JSON.parse(await readBody(request)) as ChatRequest;
  const prompt = lastUserText(body);
  prompts.push(prompt);
  const slow = prompt.includes('slow');
  const parts = slow
    ? Array.from({ length: 60 }, (_, index) => `slow part ${String(index + 1)}. `)
    : SHORT_REPLY;
  const delay = slow ? 500 : 150;
  response.writeHead(200, {
    'content-type': 'text/event-stream',
    'cache-control': 'no-cache',
    connection: 'keep-alive',
  });
  const stream = { closed: false };
  response.on('close', () => {
    stream.closed = true;
  });
  for (const part of parts) {
    if (stream.closed) return;
    response.write(chunk(part, null));
    await new Promise((resolve) => setTimeout(resolve, delay));
  }
  if (stream.closed) return;
  response.write(chunk(null, 'stop'));
  response.write(
    `data: ${JSON.stringify({
      id: 'chatcmpl-fixture',
      object: 'chat.completion.chunk',
      created: Math.floor(Date.now() / 1000),
      model: FAKE_MODEL,
      choices: [],
      usage: {
        prompt_tokens: 10,
        completion_tokens: parts.length,
        total_tokens: 10 + parts.length,
      },
    })}\n\n`,
  );
  response.end('data: [DONE]\n\n');
}
