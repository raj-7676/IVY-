#!/usr/bin/env node
// Builds the NSIS installer and puts the release together, FitGirl-repack style: one folder holding
// the small setup exe plus Ivy's two model files, each under GitHub's 2 GB per-file limit (the model
// together is ~2.4 GB, over both that limit and NSIS's 2 GB payload cap, so it can't go inside the exe).
// At install time, src-tauri/installer/hooks.nsh copies the model files from next to the setup exe into
// $INSTDIR\models\ivy-lite. Upload every file in the folder to the GitHub release; users download them
// all into one folder and run the setup.
import { copyFileSync, createReadStream, existsSync, mkdirSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = JSON.parse(execFileSync('node', ['-p', 'JSON.stringify(require("./package.json"))'], { cwd: root }).toString()).version;
const liteDir = path.join(root, 'src-tauri', 'models', 'ivy-lite');
const MODEL_FILES = ['ivy-lite-Q8_0.gguf', 'mmproj-ivy-lite-f16.gguf'];
const GITHUB_LIMIT = 2 * 1024 ** 3;

for (const f of MODEL_FILES) {
  const p = path.join(liteDir, f);
  if (!existsSync(p)) {
    console.error(`Missing ${p} — run \`npm run setup-models\` first.`);
    process.exit(1);
  }
  if (statSync(p).size >= GITHUB_LIMIT) {
    console.error(`${f} is ${(statSync(p).size / 1e9).toFixed(2)} GB, over GitHub's 2 GB per-file limit.`);
    process.exit(1);
  }
}

// Ask cargo where it really builds: CARGO_TARGET_DIR or .cargo/config.toml can move target-dir.
const { target_directory } = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: path.join(root, 'src-tauri') }).toString(),
);
const nsisDir = path.join(target_directory, 'release', 'bundle', 'nsis');

console.log('Building installer (npm run tauri build)...');
execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['run', 'tauri', 'build'], { cwd: root, stdio: 'inherit', shell: true });

const installerName = readdirSync(nsisDir).find((f) => f.endsWith('-setup.exe') && f.includes(version));
if (!installerName) {
  console.error(`No ${version} installer found in ${nsisDir}`);
  process.exit(1);
}

const out = path.join(root, 'release', `Ivy-${version}`);
mkdirSync(out, { recursive: true });
copyFileSync(path.join(nsisDir, installerName), path.join(out, installerName));
for (const f of MODEL_FILES) {
  console.log(`Copying ${f}...`);
  copyFileSync(path.join(liteDir, f), path.join(out, f));
}
// The model is Apache-2.0: its license travels with it.
copyFileSync(path.join(root, 'LICENSES', 'Apache-2.0.txt'), path.join(out, 'MODEL-LICENSE-Apache-2.0.txt'));

console.log('Hashing (SHA256SUMS.txt)...');
const sums = [];
for (const f of readdirSync(out).filter((f) => f !== 'SHA256SUMS.txt')) {
  const h = createHash('sha256');
  for await (const chunk of createReadStream(path.join(out, f))) h.update(chunk);
  sums.push(`${h.digest('hex')}  ${f}`);
}
writeFileSync(path.join(out, 'SHA256SUMS.txt'), sums.join('\n') + '\n');

console.log(`\nRelease folder ready (upload every file to the GitHub release):\n  ${out}`);
for (const f of readdirSync(out)) {
  console.log(`  ${f}  ${(statSync(path.join(out, f)).size / 1e9).toFixed(2)} GB`);
}
