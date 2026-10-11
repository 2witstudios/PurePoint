// Disposable native Pi 1.1.0 package resolution; no credentials/model/network needed.
import assert from 'node:assert/strict';
import { cp, mkdtemp, mkdir, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const piRoot = process.argv[2];
assert.ok(piRoot, 'Pass the explicit installed Pi 1.1.0 package root');
const pi = resolve(piRoot);
assert.equal(JSON.parse(await readFile(join(pi, 'package.json'), 'utf8')).version, '1.1.0');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = await mkdtemp(join(tmpdir(), 'pi-workflows-proof-'));
const previousHome = process.env.HOME;
const previousPath = process.env.PATH;
try {
  const home = join(scratch, 'empty-home');
  const cwd = join(scratch, 'project');
  const agentDir = join(home, '.pi', 'agent');
  const relocated = join(scratch, 'relocated resources', 'pointguard-workflows');
  await mkdir(agentDir, { recursive: true });
  await mkdir(cwd);
  await cp(root, relocated, { recursive: true });
  process.env.HOME = home;
  // There are no optional PageSpace/GitHub/PurePoint executables in this fixture.
  process.env.PATH = '/usr/bin:/bin';
  const load = async (file) => import(pathToFileURL(join(pi, 'dist/core', file)).href);
  const { SettingsManager } = await load('settings-manager.js');
  const { DefaultPackageManager } = await load('package-manager.js');
  const { DefaultResourceLoader } = await load('resource-loader.js');
  const { formatSkillsForPrompt } = await load('skills.js');
  const settings = SettingsManager.inMemory({ packages: [relocated, relocated] });
  const manager = new DefaultPackageManager({ cwd, agentDir, settingsManager: settings });
  const resources = await manager.resolve();
  assert.equal(resources.skills.length, 15, 'Native package manager deduplicates package sources');
  assert.ok(resources.skills.every((s) => s.enabled && s.path.startsWith(join(relocated, 'skills'))));
  assert.ok(resources.skills.every((s) => s.metadata.packageRoot === relocated));
  assert.deepEqual(resources.extensions, [], 'Workflow package executes no extension code');
  const loader = new DefaultResourceLoader({ cwd, agentDir, settingsManager: settings,
    noExtensions: true, noPromptTemplates: true, noThemes: true });
  await loader.reload();
  const { skills, diagnostics } = loader.getSkills();
  const expected = ['geo-interview', 'handoff', 'orchestrate', 'pagespace-canvas-websites',
    'pagespace-cli', 'pagespace-sdk', 'pagespace-spreadsheets', 'pagespace-task-management',
    'pagespace-writing-documents', 'plan', 'pr', 'review', 'rtc', 'task', 'triage'];
  assert.deepEqual(skills.map((s) => s.name).sort(), expected);
  assert.deepEqual(diagnostics, [], 'No collisions or malformed frontmatter');
  for (const skill of skills) {
    const text = await readFile(skill.filePath, 'utf8');
    // Every adapter link is resolvable from its relocated native base directory.
    for (const match of text.matchAll(/\]\((\.\.\/[^)]+)\)/g)) {
      const target = resolve(dirname(skill.filePath), match[1]);
      assert.ok(target.startsWith(relocated));
      assert.ok((await readFile(target)).length > 0);
    }
    assert.ok(!skill.filePath.includes('/source/'), 'Only adapters enter native discovery');
  }
  const prompt = formatSkillsForPrompt(skills);
  assert.equal((prompt.match(/<skill>/g) || []).length, 15);
  assert.ok(prompt.includes('PageSpace access needs an available authenticated'));
  assert.ok(prompt.includes('Optional AIDD guidance is not bundled'));
  assert.ok(!prompt.includes('Plan and manifest'), 'Workflow bodies are loaded progressively');
  await loader.reload();
  assert.deepEqual(loader.getSkills().skills.map((s) => s.name).sort(), expected);

  const alias = join(scratch, 'symlink package');
  await symlink(relocated, alias, 'dir');
  settings.setPackages([relocated, alias]);
  await loader.reload();
  assert.equal(loader.getSkills().skills.length, 15, 'Pi canonical skill paths deduplicate symlinks');
  assert.deepEqual(loader.getSkills().diagnostics, []);

  // Select a local override root instead of the bundled counterpart, using Pi itself.
  const override = join(scratch, 'developer override');
  await cp(root, override, { recursive: true });
  settings.setPackages([override]);
  await loader.reload();
  assert.equal(loader.getSkills().skills.length, 15);
  assert.ok(loader.getSkills().skills.every((s) => s.filePath.startsWith(override)));
  assert.deepEqual(loader.getSkills().diagnostics, []);

  // A distinct same-name user skill stays governed by Pi's native collision policy.
  const userSkill = join(agentDir, 'skills/task/SKILL.md');
  await mkdir(dirname(userSkill), { recursive: true });
  await writeFile(userSkill, '---\nname: task\ndescription: Explicit user task workflow.\n---\nUser workflow.\n');
  await loader.reload();
  assert.equal(loader.getSkills().skills.length, 15);
  assert.equal(loader.getSkills().skills.find((s) => s.name === 'task').filePath, userSkill);
  const collisions = loader.getSkills().diagnostics.filter((d) => d.type === 'collision');
  assert.equal(collisions.length, 1);
  assert.equal(collisions[0].collision.winnerPath, userSkill);
  assert.ok(collisions[0].collision.loserPath.startsWith(override));
  console.log('PASS Pi 1.1.0 native package/catalog, relocation, reload, symlink dedup, collisions and override; 15 workflows visible without optional dependencies');
} finally {
  if (previousHome === undefined) delete process.env.HOME;
  else process.env.HOME = previousHome;
  if (previousPath === undefined) delete process.env.PATH;
  else process.env.PATH = previousPath;
  await rm(scratch, { recursive: true, force: true });
}
