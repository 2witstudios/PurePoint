---
name: pagespace-sdk
description: >
  Build against the PageSpace API with the typed `@pagespace/sdk` client: auth providers, namespaces
  (drives, pages, sheets, tasks, search, agents, uploads, …), uploads, typed errors, retries and custom
  operations. Use when writing TypeScript/JavaScript that reads or writes PageSpace content programmatically.
compatibility: Requires the @pagespace/sdk package (`npm i @pagespace/sdk`). zod v4 for custom operations.
---
# @pagespace/sdk

Typed client for the PageSpace API. Methods are generated from the same operation registry as the
`pagespace` CLI and `pagespace mcp`, so a CLI verb `sheets query` is `client.sheets.queryRows`. Inputs are
validated before the request and every response is zod-validated. For a one-off from a shell, use
`/pagespace-cli` instead. Authoritative docs: packages/sdk/README.md in github.com/2witstudios/PageSpace.

```ts
import { PageSpaceClient, StaticTokenProvider } from '@pagespace/sdk';

const client = new PageSpaceClient({
  baseUrl: 'https://pagespace.ai',
  auth: new StaticTokenProvider(process.env.PAGESPACE_TOKEN!),
});
const drives = await client.drives.list({});
```

## Auth

- `StaticTokenProvider(token)` — an `mcp_` token (Settings → MCP, or `pagespace keys create --show-token`).
  Never refreshes; a rejected token fails the call without retrying it.
- `OAuthTokenProvider({ initialTokens, refreshAccessToken, onTokensUpdated })` — refreshable OAuth 2.1
  credential (what `pagespace login` stores). Auto-refreshes before expiry.
- Any `AuthProvider` (`getAccessToken()`, `invalidate()`) works.
- `client.tokens.*` needs a `ps_at_` OAuth token; an `mcp_` token gets 401 there. There is no
  `tokens.create` — keys are minted only through browser consent. Read tokens from env/secret stores;
  never hardcode or log them.

## Namespaces

`drives pages sheets tasks roles search agents conversations export tokens activity channels calendar
collaborators commands members workflows uploads workspaces`. Examples:

```ts
await client.pages.create({ driveId, title: 'Notes', type: 'DOCUMENT' });
await client.pages.read({ pageId });                       // line-numbered content
await client.pages.replaceLines({ pageId, /* start, end, content */ });
await client.sheets.queryRows({ pageId, where: { column: 'A', op: 'eq', value: 'open' } });
await client.sheets.applyFormat({ pageId, ops: [{ type: 'upsertRegion', region: { id: 'spend', range: 'A1:F', headerRows: 1 } }] });
await client.tasks.create({ pageId, title: 'Ship it' });
await client.search.glob({ driveId, pattern: '**/*.md' });
await client.agents.ask({ agentId, question: '…' });
```

Exact input shapes are in the TypeScript types — let the compiler tell you, don't guess fields.
Page types: `FOLDER DOCUMENT CHANNEL AI_CHAT CANVAS FILE SHEET TASK_LIST CODE`.

## Uploads

Use `uploadFile(client, { driveId, bytes, filename, mimeType, parentId? })`, not `client.uploads.*`
directly (it is a 3-leg flow with a binary PUT outside the SDK transport). Returns `{ page, deduplicated }`;
`deduplicated: true` is success. A 409 from presign is the cross-tenant claim guard — not dedup.

## Errors and retries

Typed subclasses of `PageSpaceError` with guards: `isRateLimitError` (`retryAfterMs`), plus
Authentication, Validation, NotFound, PermissionDenied, Server, Network, Timeout, IncompatibleServer,
ResponseValidation, Http. GETs retry with jittered backoff (network, timeout, 429, 5xx); mutations are
never replayed — so a timed-out write needs a read to learn whether it landed. Tune via `retryPolicy`.

## Custom operations

```ts
import { defineOperation } from '@pagespace/sdk';
import { z } from 'zod'; // v4
const op = defineOperation({ name: 'widgets.get', method: 'GET', path: '/api/widgets/:widgetId',
  inputSchema: z.object({ widgetId: z.string() }), outputSchema: z.object({ id: z.string() }), description: '…' });
await client.invoke(op, { widgetId: 'w1' });
```

The client checks server API version on first success against `MIN_SERVER_API_VERSION`
(`IncompatibleServerError` on mismatch) — upgrade the SDK rather than catching it.

## Page-type skills

For authoring guidance per page type see `/pagespace-canvas-websites`, `/pagespace-spreadsheets`,
`/pagespace-task-management`, `/pagespace-writing-documents`; tool→SDK names are in
`pagespace-cli/references/tool-mapping.md`.
