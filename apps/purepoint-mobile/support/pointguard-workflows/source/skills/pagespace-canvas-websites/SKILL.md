---
name: pagespace-canvas-websites
description: >
  Builds websites, landing pages, dashboards, forms and interactive pages on PageSpace CANVAS pages: HTML/CSS/JS authoring, the iframe sandbox and CSP rules, theming, linking pages, embedding uploaded files, contact/signup forms, and publishing to a public site. Use when building, styling, publishing or fixing a PageSpace CANVAS page or website.
---
> **Source:** Adapted from PageSpace's built-in `/canvas-websites` skill in the app
> (`apps/web/src/lib/ai/skills/bodies/canvas-websites.ts`). The body below was written for the in-app agent
> and names its tools. From outside the app, use the `pagespace` CLI, `@pagespace/sdk` or
> `pagespace mcp` (see /pagespace-cli, /pagespace-sdk) — the tool mapping is in the
> `pagespace-cli` skill (`references/tool-mapping.md`). Where a tool has no CLI verb
> the mapping says so.

You are building on a CANVAS page: raw HTML/CSS/JS stored as the page's content, rendered in a sandboxed iframe inside the app, and publishable as a standalone website at `https://<subdomain>.pagespace.site`.

## What a Canvas page is

- The page content IS the HTML. A shared renderer wraps it in a generated document — doctype, `<head>` (charset, viewport, title, CSP), a baseline reset (`html,body{margin:0;padding:0}`), then your markup inside a real `<body>`. The in-app iframe and the published page render from the same document, so what you see in-app is what publishes.
- Write a body FRAGMENT, not a full document. If you write a full document with an `<html>` tag, it is unwrapped: only the body content and any `<style>` blocks survive. The unwrap triggers ONLY on an `<html>` tag — a bare `<head>`/`<body>` pair without `<html>` is NOT unwrapped, and those tags land verbatim inside the rendered body. So: either a plain fragment (preferred) or a complete `<html>` document, never a partial shell. SEO/OG meta from a hand-written head is honored at publish time (see Publishing), but in-app everything else in that head is discarded.
- `<style>` blocks anywhere in your HTML are extracted, sanitized, and hoisted into the generated `<head>`, after the baseline reset — your `html`/`body` rules still win.
- `<script>` tags are preserved verbatim and execute. Isolation is by origin (the sandbox), not by a script sanitizer — write interactive JS inline in baseline mode; site mode also permits HTTPS external scripts (see the mode-specific CSP below).
- Because only the UA margin is reset, full-bleed layouts work: a `min-height:100vh` section reaches the edges with no 8px gap.

## The sandbox and what it blocks

In-app, the canvas renders in an iframe with `sandbox="allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox"` — never `allow-same-origin`. Your document is an opaque origin, walled off from the logged-in app session. Consequences:

- No PageSpace cookies or session. `fetch()` to app APIs does not inherit the logged-in session. CSP, CORS and application authorization are separate controls.
- Treat `localStorage`/`sessionStorage` as unavailable; keep state in JS variables in memory.
- No DOM access to the parent app; the only channel is `postMessage` (used by the theme bridge below).
- In-app, a `<base target="_blank">` is injected: every link without an explicit `target` opens in a new browser tab. Published pages have no base tag — links navigate normally.

CSP depends on the persisted `siteMode` flag, for both preview and publish. The baseline (`siteMode: false` or absent) carries `default-src 'none'; img-src data: https:; style-src 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; script-src 'unsafe-inline'; object-src 'none'; base-uri 'none'`. When a PageSpace app origin is configured, baseline preview and publish scope `connect-src` to that origin and permit forms to self/app origin; without it connections and form submissions are blocked. In baseline mode:

- Inline scripts run; external script hosts are blocked. External stylesheets/fonts are limited to Google Fonts.
- HTTPS/data images are allowed. Data font URLs are blocked by baseline `font-src`.
- Fetch/XHR is limited to the configured app origin, when present. This permits connection attempts, not authenticated access or only the wired-form endpoint.

Site mode uses a wider CSP in both preview and publish: HTTPS scripts/styles/fonts, data fonts, HTTPS/WSS connections, HTTPS forms and frames, and the declared blob/data asset sources are permitted. `object-src 'none'`, `base-uri 'none'`, the deny-by-default floor and the absence of `unsafe-eval` remain. Check the page's actual mode before choosing external libraries, API connections or fonts; do not assume baseline restrictions apply to site mode.

CSS sanitization also follows the page mode. Baseline mode rewrites unapproved external `url()` values to `url("")` (explicit allowed HTTPS asset hosts may be retained) and blocks external imports. Site mode preserves HTTPS CSS URLs and HTTPS `@import`; plaintext HTTP, relative and malformed URLs remain blocked. Both modes allow image/font data URI MIME types at the sanitizer, but baseline CSP still blocks data fonts. Both modes block script-execution vectors such as `expression()`, `javascript:`, `behavior:` and `data:text/html`. CSP permission alone does not override sanitizer restrictions, CORS or application authorization.

## Dark/light theming

A theme bridge script is auto-injected in BOTH contexts — never write your own. It toggles a `dark` class on `<html>`: in-app it follows the user's PageSpace theme (the app posts `{ type: 'pagespace-theme', isDark: boolean }` into the frame, and the bridge requests the current theme on load); on the published page it follows the OS `prefers-color-scheme`, live-updating on change.

Style both modes with CSS variables keyed on that class:

```css
:root { --bg: #ffffff; --fg: #111111; }
.dark { --bg: #0b0b10; --fg: #ededed; }
body { background: var(--bg); color: var(--fg); }
```

If your JS needs the theme, read `document.documentElement.classList.contains('dark')` and watch for changes with a MutationObserver on the `class` attribute.

DO: define every color as a variable with a `.dark` override.
DON'T: hardcode a single scheme, use your own `prefers-color-scheme` media queries for colors (in-app the app theme overrides the OS), or hand-roll a postMessage listener.

## Linking between pages

Write links to other PageSpace pages as in-app dashboard paths:

- Another page: `/dashboard/{driveId}/{pageId}`
- These work in-app (opening the target page in a new tab, via the injected base target) AND at publish time they are rewritten to the target's public URL: `https://<subdomain>.pagespace.site/<slug>`, or `/` when the target is the drive's home page.
- Only pages published in the SAME drive get rewritten. An unpublished or out-of-drive link is left unchanged — a dead app URL on the public site. Publish every page you link to.
- Longer paths (`/dashboard/{d}/{p}/edit` etc.) are never treated as inter-page links; only the plain form and the `/view` form are.
- External links: ordinary absolute `https://` URLs.

DO: `<a href="/dashboard/abc123/def456">Pricing</a>`
DON'T: hardcode `https://mysite.pagespace.site/pricing` for an in-drive link (breaks in-app, and breaks if the slug or subdomain changes) or link to a page you don't intend to publish.

## Embedding images and files from FILE pages

For an uploaded file (a FILE page) — image, PDF, anything — reference it as:

```html
<img src="/dashboard/{driveId}/{filePageId}/view" alt="...">
```

Never use `/api/files/...` URLs. The `/dashboard/{driveId}/{pageId}/view` form is the one convention to use — it works in `<img>`/`<a>` everywhere, with one CSS caveat:

- In-app, the app shell detects these refs and swaps them for tokenized URLs the sandboxed iframe (which has no session) can actually load — so `<img src>` and `<a href>` work in the preview.
- At publish, each referenced file is copied to a public CDN and the URL rewritten to it. The CDN host is also allowlisted through the CSS sanitizer, so a published CSS `background-image: url(/dashboard/.../view)` survives.
- CSS is mode-dependent: baseline preview can strip a file background whose resolved HTTPS host is not allowlisted; published file CDN hosts are allowlisted. Site mode permits resolved HTTPS CSS URLs in preview and publish. Relative `/view` references still need the file rewriting path; inspect the resolved URL and page mode rather than assuming all preview backgrounds must be blank. Use an `<img>` when preview portability matters.
- The same `/view` URL also works as a plain `<a href>` link to the file.
- Anything you embed this way becomes PUBLIC when the page is published — don't embed files that shouldn't be.

## Forms (waitlist, contact, signup)

For any form that should collect submissions, call the `provision_form_target` tool FIRST, with the target Sheet page id (`sheetPageId`), an ordered field list (`fields`), and — when you know which canvas the form will live on — `canvasPageId` (optional, but pass it: it lets the Forms settings tab find and manage this form later). It writes the Sheet's header row and returns `formHtml` — embed it into the canvas VERBATIM.

Field-list constraints (validated strictly; violations reject the call):

- 1 to 20 fields, each `{ name, label, type, required }`.
- `type` is exactly one of `text`, `email`, `textarea`, `checkbox` — there is NO number/tel/select/date/radio/file. For anything else, use `text` and state the expected format in the label (e.g. "Phone number", "Team size (1-10 / 11-50 / 50+)").
- `name` must match `^[a-zA-Z0-9_-]+$` (max 100 chars), be unique across the fields, and must not be `__proto__`, `constructor`, or `prototype`. It becomes the input's `name` attribute and the submitted JSON key.
- `label` becomes the Sheet's header-row column label.

- Do NOT modify the hidden honeypot input (`_hp`) or the `fetch()`-based submit script inside `formHtml`. The honeypot is deliberately off-screen via inline styles; a filled honeypot silently drops the submission. The fetch submit avoids navigating to a raw JSON response.
- The embedded submit token is safe to publish — it authorizes ONLY appending rows to that one Sheet, nothing else.
- A hand-written `<form>` will NOT submit until a human wires it in the Canvas page's Forms settings tab — there is no tool for that step. If you do hand-write one, give every input a real `name` attribute: the tab derives the field list from your markup.
- The field set is FIXED at wire time. Change the inputs before wiring; afterwards, the only path is delete-and-rewire.
- One canvas can host many forms, but each Sheet accepts only one active form.
- Test submissions on the published URL for final delivery proof. Baseline preview and publish can both permit connections to the configured app origin; site mode permits HTTPS connections. The provisioned handler uses `fetch`, so a preview attempt is possible when CSP, CORS and endpoint authorization allow it; preview success is not guaranteed. Native form submission additionally depends on iframe sandbox permissions: the current CanvasFrame includes `allow-forms`; a different preview sandbox may not. Even with that token, CSP `form-action`, destination authorization and response/navigation behavior still apply. The provisioned fetch handler prevents native navigation. Do not infer authenticated access or successful submission from CSP allowance alone.
- Optional: an element with `data-role="form-status"` inside the form shows submit status messages.

DO: provision first, paste `formHtml` unchanged, style it via CSS around/atop it.
DON'T: rewrite the form's script "more cleanly", remove or label `_hp`, or promise a hand-written form works without human wiring.

## Multi-page site structure

A published drive is one site on one subdomain: the drive's home page at `/`, every other published canvas at `/<slug>`.

- Organize site pages as a parent canvas (home / nav hub) with child canvas pages per section — any page can nest under any page.
- There is no shared template or include mechanism: repeat your `<nav>` block (of `/dashboard/...` links) on each page; publish rewrites the links per page.
- Keep each canvas focused — one screen, one job; use the home canvas as the navigation hub.

## Publishing

Publishing renders the canvas to a standalone HTML artifact served at `https://<subdomain>.pagespace.site/<path>`:

- The subdomain belongs to the drive and, once allocated, is always reused. On the drive's FIRST publish it is allocated from the drive slug (or an explicitly passed candidate) — and when that value is taken, reserved, or malformed it is auto-renamed to a unique variant (`acme` → `acme-2`) rather than erroring. After allocation, a subdomain passed with a publish is ignored. The path is slugified from the page title unless overridden. The drive home page is also served at the site root.
- The published head gets full SEO/social treatment: canonical URL, meta description (derived from the page's first text when not set), robots (default `index, follow`; a noindex option exists), Open Graph, Twitter Card, and JSON-LD tags.
- You can control that meta from the canvas itself: a `<title>`, `<meta name="description">`, `<meta property="og:title|og:description|og:image">`, or `<link rel="icon">` you write in the HTML is extracted at publish and hoisted into the head, winning over UI-set fallbacks.
- The artifact is a snapshot rendered at publish time — republish after content edits to update the live site.

## Common pitfalls

- Check `siteMode`: baseline external scripts are blocked and styling hosts are limited; site mode permits HTTPS scripts/styles/fonts.
- Baseline connections are restricted to the configured app origin, or blocked without one; site mode permits HTTPS/WSS connections in preview and publish. CORS and application authorization still apply. Prefer wired forms for supported public submissions and never assume a logged-in session.
- DON'T rely on `localStorage`, cookies, or cross-page JS state; each page is standalone and the origin is opaque.
- DON'T use `/api/files/...` for file embeds — always `/dashboard/{driveId}/{filePageId}/view`.
- DON'T write full `<html>` documents expecting the head to render in-app; write fragments and let publish-time extraction handle meta.
- Baseline CSS blocks unapproved external URLs/imports; site mode preserves HTTPS CSS URLs/imports. Use data image URIs or `<img>` where baseline restrictions apply.
- For a blank CSS file background, check the resolved URL, asset host allowance and page mode in preview and publish; relative URLs may be blocked by sanitization.
- DON'T edit provisioned `formHtml`, and DON'T leave links pointing at pages that won't be published.
