// Tests for the WorkspacePersistence.swift port (WIN-6.3).
//
// The golden fixtures in docs/reference/fixtures/workspaces/ are JSONEncoder output with the
// options WorkspacePersistence.swift uses. The same files are checked against the real Swift
// code by purepoint-macosTests/WorkspaceFixtureTests.swift, so a byte-identical round trip here
// means the two apps write the same file.

import gridLayout from '../../../../../docs/reference/fixtures/workspaces/grid-layout.json?raw';
import gridLayoutExpected from '../../../../../docs/reference/fixtures/workspaces/grid-layout.expected.json?raw';
import v2 from '../../../../../docs/reference/fixtures/workspaces/v2.json?raw';
import v2Expected from '../../../../../docs/reference/fixtures/workspaces/v2.expected.json?raw';
import v3 from '../../../../../docs/reference/fixtures/workspaces/v3.json?raw';
import v3Expected from '../../../../../docs/reference/fixtures/workspaces/v3.expected.json?raw';
import v4 from '../../../../../docs/reference/fixtures/workspaces/v4.json?raw';
import { compareSwiftKeys, encodeSwiftJson, swiftNumber, swiftString } from './swiftJson';
import {
  POINT_GUARD_CONTENT,
  PROJECT_ROOT,
  adoptingWorkspace,
  agentContent,
  isPointGuard,
  pointGuardId,
  pointGuardWorkspace,
  type Workspace,
} from './workspace';
import {
  LayoutWriteConflictError,
  decodeWorkspaceDocument,
  decodedWorkspaces,
  documentText,
  isNewerThanSupported,
  legacyGridLayoutPath,
  loadRemoteLayout,
  loadWorkspaces,
  loadWorkspacesForRegistry,
  memoryWorkspaceFileIO,
  probeDocumentVersion,
  saveLocalLayout,
  saveRemoteLayout,
  saveWorkspaces,
  workspaceFilePath,
  workspacesFromText,
  type HostFileText,
  type LayoutHostIO,
} from './workspacePersistence';
import {
  WORKSPACE_SELECTION_KEY,
  loadWorkspaceSelection,
  saveWorkspaceSelection,
} from './workspaceSelection';

/** The project the fixtures were written for: the Point Guard workspace ID embeds it. */
const FIXTURE_PROJECT = '/Users/dev/project';
const ROOT = '/tmp/project';

/** Decode a document and encode it again the way the Mac's `save` would. */
function reencode(text: string): string | null {
  const document = decodeWorkspaceDocument(text);
  return documentText(
    workspacesFromText(text, FIXTURE_PROJECT),
    document?.activeWorkspaceId ?? null,
  );
}

describe('swiftJson (JSONEncoder [.prettyPrinted, .sortedKeys])', () => {
  it('givenNestedValuesShouldIndentTwoSpacesWithASpaceBeforeEachColon', () => {
    expect(encodeSwiftJson({ b: [1, { d: true, c: null }], a: 'x' })).toBe(
      '{\n  "a" : "x",\n  "b" : [\n    1,\n    {\n      "c" : null,\n      "d" : true\n    }\n  ]\n}',
    );
  });

  it('givenEmptyContainersShouldWriteThemOnOneLine', () => {
    expect(encodeSwiftJson({ a: [], b: {} })).toBe('{\n  "a" : [],\n  "b" : {}\n}');
  });

  it('givenUndefinedMembersShouldOmitThemLikeEncodeIfPresent', () => {
    expect(encodeSwiftJson({ a: 1, b: undefined })).toBe('{\n  "a" : 1\n}');
  });

  it('givenStringsShouldEscapeSlashesQuotesAndControlCharactersOnly', () => {
    expect(swiftString('/a "b" \\ \n\t\r\b\f \u0001 – é')).toBe(
      '"\\/a \\"b\\" \\\\ \\n\\t\\r\\b\\f \\u0001 – é"',
    );
  });

  it('givenNumbersShouldPrintLikeSwiftDoubleDescriptionWithoutTrailingZero', () => {
    expect(swiftNumber(4)).toBe('4');
    expect(swiftNumber(1.0)).toBe('1');
    expect(swiftNumber(-3)).toBe('-3');
    expect(swiftNumber(0.5)).toBe('0.5');
    expect(swiftNumber(1 / 3)).toBe('0.3333333333333333');
    expect(swiftNumber(0.1 + 0.2)).toBe('0.30000000000000004');
    expect(swiftNumber(123.456)).toBe('123.456');
    expect(swiftNumber(0.0001)).toBe('0.0001');
    expect(swiftNumber(0.00001)).toBe('1e-05');
    expect(swiftNumber(-1.5e-7)).toBe('-1.5e-07');
    expect(() => swiftNumber(Number.NaN)).toThrow();
  });

  it('givenKeysShouldSortNumericallyAndIgnoringCase', () => {
    expect(['b', 'B', 'a10', 'a2', 'nextSurfaceId', 'nextLeafId'].sort(compareSwiftKeys)).toEqual([
      'a2',
      'a10',
      'B',
      'b',
      'nextLeafId',
      'nextSurfaceId',
    ]);
  });
});

describe('WorkspacePersistence golden fixtures', () => {
  it('givenVersionFourDocumentShouldReencodeByteIdentically', () => {
    expect(reencode(v4)).toBe(v4);
  });

  it('givenVersionFourDocumentShouldKeepPointGuardTabs', () => {
    const [, pointGuard] = workspacesFromText(v4, FIXTURE_PROJECT);
    expect(pointGuard?.id).toBe(pointGuardId(FIXTURE_PROJECT));
    expect(pointGuard?.panes[0]?.tabs.map((tab) => tab.content)).toEqual([
      POINT_GUARD_CONTENT,
      agentContent('ag-c'),
    ]);
  });

  it('givenVersionThreeDocumentShouldLoadAndSaveAsVersionFour', () => {
    expect(reencode(v3)).toBe(v3Expected);
  });

  it('givenVersionTwoDocumentShouldMigrateToTheExpectedVersionFour', () => {
    expect(reencode(v2)).toBe(v2Expected);
  });

  it('givenLegacyGridLayoutShouldMigrateToTheExpectedVersionFour', async () => {
    const io = memoryWorkspaceFileIO(new Map([[legacyGridLayoutPath(ROOT), gridLayout]]));

    await loadWorkspaces(io, ROOT);

    expect(io.files.get(workspaceFilePath(ROOT))).toBe(gridLayoutExpected);
  });

  it('givenSavedWorkspacesShouldWriteWhatTheyDecodeBackTo', () => {
    const workspaces = workspacesFromText(v4, FIXTURE_PROJECT);
    expect(decodedWorkspaces(workspaces, FIXTURE_PROJECT)).toEqual(workspaces);
  });
});

describe('WorkspacePersistence', () => {
  it('givenFilePathShouldBeUnderTheProjectsPuDirectory', () => {
    expect(workspaceFilePath('/tmp/project')).toBe('/tmp/project/.pu/workspaces.json');
    expect(workspaceFilePath('/tmp/project/')).toBe('/tmp/project/.pu/workspaces.json');
    expect(legacyGridLayoutPath('C:\\dev\\project')).toBe('C:\\dev\\project/.pu/grid-layout.json');
  });

  it('givenUnknownTabKindShouldLoadItAsAnEmptyTabRatherThanFail', () => {
    const text = v3.replace('"kind" : "file"', '"kind" : "browser"');
    const tabs = workspacesFromText(text, ROOT)[0]?.panes[0]?.tabs;
    expect(tabs?.map((tab) => tab.content.kind)).toEqual(['agent', 'empty']);
  });

  it('givenFieldOfTheWrongTypeShouldRejectTheWholeDocumentLikeJSONDecoder', () => {
    expect(
      workspacesFromText(v3.replace('"worktreeId" : "wt-1"', '"worktreeId" : 1'), ROOT),
    ).toEqual([]);
    expect(
      workspacesFromText(v3.replace('"axis" : "vertical"', '"axis" : "diagonal"'), ROOT),
    ).toEqual([]);
    expect(workspacesFromText('not json', ROOT)).toEqual([]);
  });

  it('givenExplicitNullOptionalShouldDecodeAsAbsent', () => {
    const text = v3.replace('"worktreeId" : "wt-1"', '"worktreeId" : null');
    expect(workspacesFromText(text, ROOT)[0]?.container).toEqual(PROJECT_ROOT);
  });

  it('givenAnyDocumentShouldProbeItsVersionBeforeDecoding', () => {
    expect(probeDocumentVersion('{"version":99,"workspaces":"something else"}')).toBe(99);
    expect(probeDocumentVersion('{"workspaces":[]}')).toBeNull();
    expect(isNewerThanSupported('{"version":5}')).toBe(true);
    expect(isNewerThanSupported('{"version":4}')).toBe(false);
    expect(isNewerThanSupported('garbage')).toBe(false);
    expect(isNewerThanSupported(null)).toBe(false);
  });

  it('givenPointGuardWorkspaceFromAnotherProjectKeyShouldRekeyItAndPersistTheNewId', async () => {
    const io = memoryWorkspaceFileIO(new Map([[workspaceFilePath(ROOT), v4]]));

    const restored = await loadWorkspacesForRegistry(io, ROOT);

    expect(restored[1]?.id).toBe(pointGuardId(ROOT));
    const written = io.files.get(workspaceFilePath(ROOT)) ?? '';
    expect(written).toContain('"id" : "ws-point-guard:\\/tmp\\/project"');
    expect(written).toContain('"activeWorkspaceId" : "ws-ag-a"');
    expect(written).toContain('"version" : 4');
  });

  it('givenNewerDocumentNeedingAnIdentityMigrationShouldPatchItAndKeepUnknownFields', async () => {
    const newer = JSON.stringify({
      version: 9,
      future: { keep: true },
      activeWorkspaceId: 'ws-point-guard:/elsewhere',
      workspaces: [
        {
          id: 'ws-point-guard:/elsewhere',
          focusedLeafId: 0,
          nextLeafId: 1,
          tree: { type: 'leaf', leafId: 0, tabs: [{ id: 0, kind: 'hologram' }] },
        },
      ],
    });
    const io = memoryWorkspaceFileIO(new Map([[workspaceFilePath(ROOT), newer]]));

    await loadWorkspacesForRegistry(io, ROOT);

    const written = JSON.parse(io.files.get(workspaceFilePath(ROOT)) ?? '{}') as Record<
      string,
      unknown
    >;
    expect(written.version).toBe(9);
    expect(written.future).toEqual({ keep: true });
    expect(written.activeWorkspaceId).toBe(pointGuardId(ROOT));
    expect(JSON.stringify(written.workspaces)).toContain('hologram');
  });

  it('givenIdentityMigrationThatCannotBeWrittenShouldFailTheRegistryLoad', async () => {
    const io = memoryWorkspaceFileIO(new Map([[workspaceFilePath(ROOT), v4]]));
    io.write = () => Promise.reject(new Error('disk full'));

    await expect(loadWorkspacesForRegistry(io, ROOT)).rejects.toThrow('disk full');
    await expect(loadWorkspaces(io, ROOT)).resolves.toEqual([]);
  });

  it('givenUnreadableFileShouldLoadEmptyRatherThanFail', async () => {
    const io = memoryWorkspaceFileIO();
    io.read = () => Promise.reject(new Error('EACCES'));
    await expect(loadWorkspaces(io, ROOT)).resolves.toEqual([]);
  });

  it('givenLegacyLayoutWithNoAgentShouldRetireItAndLoadNothing', async () => {
    const io = memoryWorkspaceFileIO(
      new Map([[legacyGridLayoutPath(ROOT), '{"type":"leaf","leafId":0}']]),
    );

    expect(await loadWorkspaces(io, ROOT)).toEqual([]);
    expect(io.files.size).toBe(0);
  });

  it('givenBareLegacyTreeShouldAnchorOnItsFirstAgent', async () => {
    const bare =
      '{"type":"split","axis":"vertical","ratio":0.5,"first":{"type":"leaf"},"second":{"type":"leaf","agentId":"ag-b"}}';
    const io = memoryWorkspaceFileIO(new Map([[legacyGridLayoutPath(ROOT), bare]]));

    const [workspace] = await loadWorkspaces(io, ROOT);

    expect(workspace?.id).toBe('ws-ag-b');
    expect(io.files.has(legacyGridLayoutPath(ROOT))).toBe(false);
  });

  it('givenPointGuardWorkspaceShouldSaveItsTabAsPointGuard', async () => {
    const io = memoryWorkspaceFileIO();
    await saveWorkspaces(io, ROOT, [pointGuardWorkspace(ROOT)]);
    const [restored] = await loadWorkspaces(io, ROOT);
    expect(restored !== undefined && isPointGuard(restored)).toBe(true);
    expect(restored?.panes[0]?.tabs[0]?.content).toEqual(POINT_GUARD_CONTENT);
  });
});

// MARK: - Conditional saves

/** A host over one in-memory file with a modification clock. */
function fakeHost(initial: { text: string; modifiedMs: number } | null = null) {
  let file = initial;
  let clock = 100;
  const host: LayoutHostIO & { file: () => typeof file; touch: (text: string) => void } = {
    file: () => file,
    touch: (text) => {
      clock += 1;
      file = { text, modifiedMs: clock };
    },
    stat: () => Promise.resolve(file === null ? null : { modifiedMs: file.modifiedMs }),
    read: () =>
      Promise.resolve<HostFileText | null>(file === null ? null : { ...file, truncated: false }),
    write: (_path, text, expected) => {
      if ((file?.modifiedMs ?? null) !== expected) {
        return Promise.reject(new LayoutWriteConflictError());
      }
      clock += 1;
      file = { text, modifiedMs: clock };
      return Promise.resolve(clock);
    },
  };
  return host;
}

describe('WorkspacePersistence conditional saves', () => {
  const text = documentText([adoptingWorkspace('ag-a', PROJECT_ROOT)]) ?? '';

  for (const [name, save] of [
    ['saveLocal', saveLocalLayout],
    ['saveRemote', saveRemoteLayout],
  ] as const) {
    it(`${name}: givenNoDocumentShouldWriteOne`, async () => {
      const host = fakeHost();
      expect(await save(host, text, ROOT, null)).toEqual({ kind: 'written', modifiedMs: 101 });
      expect(host.file()?.text).toBe(text);
    });

    it(`${name}: givenUnchangedDocumentShouldOverwriteIt`, async () => {
      const host = fakeHost({ text: v3, modifiedMs: 50 });
      expect((await save(host, text, ROOT, 50)).kind).toBe('written');
    });

    it(`${name}: givenDocumentChangedElsewhereShouldReturnItInsteadOfOverwriting`, async () => {
      const host = fakeHost({ text: v3, modifiedMs: 60 });
      expect(await save(host, text, ROOT, 50)).toEqual({
        kind: 'conflict',
        text: v3,
        modifiedMs: 60,
      });
      expect(host.file()?.text).toBe(v3);
    });

    it(`${name}: givenNewerDocumentShouldSkipEvenWhenUnchanged`, async () => {
      const newer = '{"version":5,"workspaces":[]}';
      const host = fakeHost({ text: newer, modifiedMs: 50 });
      expect(await save(host, text, ROOT, 50)).toEqual({ kind: 'skipped' });
      expect(await save(host, text, ROOT, 49)).toEqual({ kind: 'skipped' });
      expect(host.file()?.text).toBe(newer);
    });

    it(`${name}: givenDocumentDeletedSinceReadShouldWriteItAgain`, async () => {
      const host = fakeHost();
      expect((await save(host, text, ROOT, 50)).kind).toBe('written');
    });

    it(`${name}: givenUnreadableDocumentShouldSkip`, async () => {
      const host = fakeHost({ text: v3, modifiedMs: 50 });
      host.stat = () => Promise.reject(new Error('EIO'));
      host.read = () => Promise.reject(new Error('EIO'));
      expect(await save(host, text, ROOT, 50)).toEqual({ kind: 'skipped' });
    });
  }

  it('saveRemote: givenDocumentChangedBetweenReadAndWriteShouldAdoptIt', async () => {
    const host = fakeHost({ text: v3, modifiedMs: 50 });
    const write = host.write.bind(host);
    host.write = (path, body, expected) => {
      host.touch(v4);
      return write(path, body, expected);
    };
    expect(await saveRemoteLayout(host, text, ROOT, 50)).toEqual({
      kind: 'conflict',
      text: v4,
      modifiedMs: 101,
    });
  });

  it('loadRemote: givenMissingDocumentShouldLoadEmptyAndTruncatedShouldFail', async () => {
    expect(await loadRemoteLayout(fakeHost(), ROOT, ROOT)).toEqual({
      workspaces: [],
      modifiedMs: null,
    });
    const host = fakeHost({ text: v3, modifiedMs: 7 });
    expect((await loadRemoteLayout(host, ROOT, ROOT)).workspaces).toHaveLength(1);
    host.read = () => Promise.resolve({ text: v3.slice(0, 10), modifiedMs: 7, truncated: true });
    await expect(loadRemoteLayout(host, ROOT, ROOT)).rejects.toThrow('larger than 4 MB');
  });
});

// MARK: - Selection

describe('workspaceSelection', () => {
  function memoryStorage() {
    const items = new Map<string, string>();
    return {
      items,
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => void items.set(key, value),
      removeItem: (key: string) => void items.delete(key),
    };
  }

  it('givenSelectedWorkspaceShouldRestoreItFromTheAppsOwnStore', () => {
    const storage = memoryStorage();
    saveWorkspaceSelection('ws-ag-a', storage);
    expect([...storage.items.keys()]).toEqual([WORKSPACE_SELECTION_KEY]);
    expect(loadWorkspaceSelection(storage)).toBe('ws-ag-a');
  });

  it('givenNoSelectionShouldForgetTheSavedOne', () => {
    const storage = memoryStorage();
    saveWorkspaceSelection('ws-ag-a', storage);
    saveWorkspaceSelection(null, storage);
    expect(loadWorkspaceSelection(storage)).toBeNull();
  });

  it('givenBrokenOrMissingStorageShouldReadAsNothingSaved', () => {
    const storage = memoryStorage();
    storage.items.set(WORKSPACE_SELECTION_KEY, '{not json');
    expect(loadWorkspaceSelection(storage)).toBeNull();
    expect(loadWorkspaceSelection(null)).toBeNull();
    const throwing = {
      getItem: () => {
        throw new Error('SecurityError');
      },
      setItem: () => {
        throw new Error('QuotaExceededError');
      },
      removeItem: () => undefined,
    };
    expect(() => {
      saveWorkspaceSelection('ws-ag-a', throwing);
    }).not.toThrow();
    expect(loadWorkspaceSelection(throwing)).toBeNull();
  });

  it('givenSelectionShouldNeverReachTheSharedLayoutFile', () => {
    const workspace: Workspace = adoptingWorkspace('ag-a', PROJECT_ROOT);
    expect(documentText([workspace])).not.toContain('activeWorkspaceId');
  });
});
