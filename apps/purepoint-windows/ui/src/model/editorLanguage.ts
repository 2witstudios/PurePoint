// `EditorLanguage` from apps/purepoint-macos/purepoint-macos/Models/EditorTab.swift (WIN-8.3, for
// the diff highlighter): a file's language from its name. The rest of EditorTab.swift (the tab
// model, `icon`) is WIN-8.5, which imports this.

export const EDITOR_LANGUAGES = [
  'swift',
  'rust',
  'javascript',
  'typescript',
  'tsx',
  'python',
  'markdown',
  'json',
  'yaml',
  'toml',
  'html',
  'css',
  'shell',
  'c',
  'cpp',
  'go',
  'ruby',
  'php',
  'java',
  'csharp',
  'scala',
  'haskell',
  'lua',
  'xml',
  'plaintext',
] as const;
export type EditorLanguage = (typeof EDITOR_LANGUAGES)[number];

const KNOWN_FILENAMES: Readonly<Record<string, EditorLanguage>> = {
  // Lock files
  'cargo.lock': 'toml',
  'package.resolved': 'json',
  'composer.lock': 'json',
  // Ruby conventions
  gemfile: 'ruby',
  rakefile: 'ruby',
  podfile: 'ruby',
  vagrantfile: 'ruby',
  fastfile: 'ruby',
  brewfile: 'ruby',
  dangerfile: 'ruby',
  guardfile: 'ruby',
  // Build/task files
  makefile: 'shell',
  gnumakefile: 'shell',
  dockerfile: 'shell',
  justfile: 'shell',
  procfile: 'shell',
  // Ignore files
  '.gitignore': 'shell',
  '.dockerignore': 'shell',
  '.npmignore': 'shell',
  '.hgignore': 'shell',
  // Config dotfiles
  '.editorconfig': 'plaintext',
  '.gitconfig': 'plaintext',
  '.gitmodules': 'plaintext',
  // Shell dotfiles
  '.zshrc': 'shell',
  '.bashrc': 'shell',
  '.bash_profile': 'shell',
  '.profile': 'shell',
  '.zprofile': 'shell',
  '.zshenv': 'shell',
  '.bash_aliases': 'shell',
  '.bash_logout': 'shell',
};

const EXTENSIONS: Readonly<Record<string, EditorLanguage>> = {
  swift: 'swift',
  rs: 'rust',
  js: 'javascript',
  jsx: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  tsx: 'tsx',
  py: 'python',
  pyi: 'python',
  md: 'markdown',
  markdown: 'markdown',
  mdx: 'markdown',
  json: 'json',
  jsonc: 'json',
  json5: 'json',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'toml',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'css',
  sass: 'css',
  less: 'css',
  sh: 'shell',
  bash: 'shell',
  zsh: 'shell',
  fish: 'shell',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',
  hxx: 'cpp',
  hh: 'cpp',
  go: 'go',
  rb: 'ruby',
  rake: 'ruby',
  gemspec: 'ruby',
  php: 'php',
  java: 'java',
  cs: 'csharp',
  scala: 'scala',
  sc: 'scala',
  hs: 'haskell',
  lhs: 'haskell',
  lua: 'lua',
  xml: 'xml',
  xsl: 'xml',
  xslt: 'xml',
  svg: 'xml',
  plist: 'xml',
  graphql: 'plaintext',
  gql: 'plaintext',
  dockerfile: 'shell',
};

/** Swift `EditorLanguage.detect(from:)`: known file names first, then the extension. */
export function detectEditorLanguage(filename: string): EditorLanguage {
  const name = (filename.split(/[\\/]/).pop() ?? filename).toLowerCase();
  const known = KNOWN_FILENAMES[name];
  if (known !== undefined) return known;
  // NSString.pathExtension: none for a leading-dot name such as `.env`.
  const dot = name.lastIndexOf('.');
  const extension = dot > 0 ? name.slice(dot + 1) : '';
  return EXTENSIONS[extension] ?? 'plaintext';
}
