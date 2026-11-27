// node --test scripts/stage-pointguard-runtime.test.mjs (part of `npm test`): which bridge files
// the staged Point Guard runtime ships (WIN-9.3). Tests and their helpers never ship.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';

import { MOBILE, shipsInRuntime } from './stage-pointguard-runtime.mjs';

test('ships bridge sources but no tests or test helpers', () => {
  assert.equal(shipsInRuntime('bridge/main.js'), true);
  assert.equal(shipsInRuntime('bridge/acl.js'), true);
  assert.equal(shipsInRuntime('bridge/acl.test.js'), false);
  assert.equal(shipsInRuntime('bridge/private-temp.test-helper.js'), false);
  assert.equal(shipsInRuntime('bridge/latest.js'), true);
});

test('every bridge test helper is named so it is left out', () => {
  const bridge = path.join(MOBILE, 'bridge');
  const sources = fs.readdirSync(bridge).filter((name) => name.endsWith('.js'));
  const shipped = sources.filter((name) => shipsInRuntime(path.join(bridge, name)));
  const shippedSet = new Set(shipped);
  // A shipped module must not be imported only by tests: anything a test imports that no
  // shipped module imports is a test helper and must be named *.test-helper.js.
  const importsOf = (name) =>
    [...fs.readFileSync(path.join(bridge, name), 'utf8').matchAll(/from "\.\/([^"]+\.js)"/g)].map(
      (m) => m[1],
    );
  const usedByRuntime = new Set(shipped.flatMap(importsOf));
  const usedByTests = new Set(sources.filter((n) => n.endsWith('.test.js')).flatMap(importsOf));
  const testOnly = [...usedByTests].filter(
    (name) => shippedSet.has(name) && !usedByRuntime.has(name) && name !== 'main.js',
  );
  assert.deepEqual(
    testOnly.filter((name) => /temp|fixture|helper|fake/i.test(name)),
    [],
    `test-only helpers that would ship: ${testOnly.join(', ')}`,
  );
  assert.ok(!shipped.includes('private-temp.js'));
});
