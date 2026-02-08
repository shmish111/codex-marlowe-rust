import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { basename, extname, resolve } from 'node:path';

const repoRoot = resolve(new URL('.', import.meta.url).pathname, '..');
const examplesDir = resolve(repoRoot, '..', 'examples');
const publicDir = resolve(repoRoot, 'public', 'examples');
const metaPath = resolve(repoRoot, 'examples.meta.json');

const metaRaw = await readFile(metaPath, 'utf-8');
const meta = JSON.parse(metaRaw);

const files = (await readdir(examplesDir)).filter((file) => extname(file) === '.yaml');

await mkdir(publicDir, { recursive: true });

const index = [];

for (const file of files) {
  const filePath = resolve(examplesDir, file);
  const code = await readFile(filePath, 'utf-8');
  const baseName = basename(file, '.yaml');
  const displayName = meta[file]?.name ?? baseName.replace(/-/g, ' ');
  const description = meta[file]?.description ?? 'Example contract from the repository.';

  const targetPath = resolve(publicDir, file);
  await writeFile(targetPath, code);

  index.push({
    id: baseName,
    name: displayName,
    description,
    language: 'yaml',
    file: `/examples/${file}`,
    code
  });
}

index.sort((a, b) => a.name.localeCompare(b.name));

await writeFile(resolve(publicDir, 'index.json'), JSON.stringify(index, null, 2));

console.log(`Synced ${index.length} examples to ${publicDir}`);
