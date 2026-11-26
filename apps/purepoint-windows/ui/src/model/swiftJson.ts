// A JSON serializer that writes what Apple's `JSONEncoder` writes with
// `outputFormatting = [.prettyPrinted, .sortedKeys]` (WIN-6.3).
//
// `.pu/workspaces.json` is shared with the Mac app, which writes it that way
// (Views/PaneGrid/WorkspacePersistence.swift). Writing the same bytes keeps a document that
// round-trips through both apps from churning (and keeps diffs of a checked-in layout quiet).
// The rules, checked against the golden fixtures in docs/reference/fixtures/workspaces/:
// - two-space indent, one member per line, `"key" : value` (a space on both sides of the colon);
// - an empty array or object is `[]` / `{}`;
// - keys sorted the way Foundation sorts them for `.sortedKeys` (numeric, case-insensitive,
//   then ordinal to break ties);
// - strings escape `"`, `\`, `/` (as `\/`, since `.withoutEscapingSlashes` is not set),
//   `\b \f \n \r \t`, other control characters as `\u00xx`; everything else is raw UTF-8;
// - integers print plainly; other numbers print as Swift's `Double.description`, without a
//   trailing `.0` (shortest round-trip digits, exponent form below 1e-4: `1e-05`);
// - no trailing newline.
//
// Codable never encodes `nil` (synthesized `encodeIfPresent` drops the key), so a `null` here only
// comes from patching raw JSON, and prints as `null` like `JSONSerialization` does.

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue | undefined };

/** `undefined` members are omitted, like a `nil` optional under `encodeIfPresent`. */
export function encodeSwiftJson(value: JsonValue): string {
  return write(value, 0);
}

function write(value: JsonValue, depth: number): string {
  if (value === null) return 'null';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'number') return swiftNumber(value);
  if (typeof value === 'string') return swiftString(value);
  const indent = '  '.repeat(depth + 1);
  const closing = '  '.repeat(depth);
  if (isArray(value)) {
    if (value.length === 0) return '[]';
    const items = value.map((item) => indent + write(item, depth + 1));
    return `[\n${items.join(',\n')}\n${closing}]`;
  }
  const keys = Object.keys(value)
    .filter((key) => value[key] !== undefined)
    .sort(compareSwiftKeys);
  if (keys.length === 0) return '{}';
  const members = keys.map(
    (key) => `${indent}${swiftString(key)} : ${write(value[key] as JsonValue, depth + 1)}`,
  );
  return `{\n${members.join(',\n')}\n${closing}}`;
}

function isArray(value: JsonValue): value is readonly JsonValue[] {
  return Array.isArray(value);
}

// MARK: - Numbers

/**
 * Swift's `JSONEncoder` throws on non-finite numbers (the default `nonConformingFloatEncodingStrategy`),
 * so a document holding one is never written.
 */
export class NonConformingNumberError extends Error {
  constructor(value: number) {
    super(`JSONEncoder cannot encode ${String(value)}`);
    this.name = 'NonConformingNumberError';
  }
}

export function swiftNumber(value: number): string {
  if (!Number.isFinite(value)) throw new NonConformingNumberError(value);
  // Int fields print plainly, and so does a whole Double once its trailing ".0" is dropped.
  if (Number.isInteger(value) && Math.abs(value) < 2 ** 53) return String(value);
  // `toExponential()` with no argument gives the shortest digits that round-trip, which is what
  // Swift's `description` prints too; only the layout differs from JavaScript's `String()`.
  const match = /^(-?)(\d)(?:\.(\d+))?e([+-]\d+)$/.exec(value.toExponential());
  if (match === null) return String(value);
  const [, sign = '', lead = '', rest = '', exponentText = '0'] = match;
  const exponent = Number(exponentText);
  const digits = lead + rest;
  if (exponent < -4 || exponent >= 16) {
    const mantissa = rest === '' ? lead : `${lead}.${rest}`;
    const magnitude = String(Math.abs(exponent)).padStart(2, '0');
    return `${sign}${mantissa}e${exponent < 0 ? '-' : '+'}${magnitude}`;
  }
  if (exponent < 0) return `${sign}0.${'0'.repeat(-exponent - 1)}${digits}`;
  const whole = digits.slice(0, exponent + 1).padEnd(exponent + 1, '0');
  const fraction = digits.slice(exponent + 1);
  return fraction === '' ? `${sign}${whole}` : `${sign}${whole}.${fraction}`;
}

// MARK: - Strings

const ESCAPES: Readonly<Record<string, string>> = {
  '"': '\\"',
  '\\': '\\\\',
  '/': '\\/',
  '\b': '\\b',
  '\f': '\\f',
  '\n': '\\n',
  '\r': '\\r',
  '\t': '\\t',
};

export function swiftString(value: string): string {
  let out = '"';
  for (const char of value) {
    const escaped = ESCAPES[char];
    if (escaped !== undefined) {
      out += escaped;
    } else if (char.charCodeAt(0) < 0x20) {
      out += '\\u' + char.charCodeAt(0).toString(16).padStart(4, '0');
    } else {
      out += char;
    }
  }
  return out + '"';
}

// MARK: - Key order

/**
 * Foundation's `.sortedKeys` order: `compare(_:options: [.numeric, .caseInsensitive,
 * .forcedOrdering])`. Runs of digits compare by value, letters ignoring case, and an otherwise
 * equal pair falls back to an ordinal comparison so the order is total.
 */
export function compareSwiftKeys(a: string, b: string): number {
  const tokensA = keyTokens(a);
  const tokensB = keyTokens(b);
  const count = Math.min(tokensA.length, tokensB.length);
  for (let i = 0; i < count; i++) {
    const order = compareTokens(tokensA[i] ?? '', tokensB[i] ?? '');
    if (order !== 0) return order;
  }
  if (tokensA.length !== tokensB.length) return tokensA.length - tokensB.length;
  return a < b ? -1 : a > b ? 1 : 0;
}

function keyTokens(key: string): string[] {
  return key.match(/\d+|\D/gu) ?? [];
}

function compareTokens(a: string, b: string): number {
  const digitsA = /^\d+$/.test(a);
  const digitsB = /^\d+$/.test(b);
  if (digitsA && digitsB) {
    const trimmedA = a.replace(/^0+(?=\d)/, '');
    const trimmedB = b.replace(/^0+(?=\d)/, '');
    if (trimmedA.length !== trimmedB.length) return trimmedA.length - trimmedB.length;
    return trimmedA < trimmedB ? -1 : trimmedA > trimmedB ? 1 : 0;
  }
  const lowerA = a.toLowerCase();
  const lowerB = b.toLowerCase();
  return lowerA < lowerB ? -1 : lowerA > lowerB ? 1 : 0;
}
