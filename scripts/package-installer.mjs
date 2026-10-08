#!/usr/bin/env node
// Builds the NSIS installer and puts the release together, FitGirl-repack style: one folder holding
// the small setup exe plus Ivy's two model files, each under GitHub's 2 GB per-file limit (the model
// together is ~2.4 GB, over both that limit and NSIS's 2 GB payload cap, so it can't go inside the exe).
// At install time, src-tauri/installer/hooks.nsh copies the model files from next to the setup exe into
// $INSTDIR\models\ivy-lite (an offline install); otherwise Ivy downloads them on first start
// (src-tauri/src/model.rs, from the v0.2.6 release). Upload every file in the folder to the GitHub release.
import { copyFileSync, createReadStream, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
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

const sha256 = async (file) => {
  const h = createHash('sha256');
  for await (const chunk of createReadStream(file)) h.update(chunk);
  return h.digest('hex');
};

// hooks.nsh and model.rs pin each model file's size and SHA-256 (anything that doesn't match is deleted),
// so a typo there would make every install fail. Check them against the real files before building.
const hooks = readFileSync(path.join(root, 'src-tauri', 'installer', 'hooks.nsh'), 'utf8');
const modelRs = readFileSync(path.join(root, 'src-tauri', 'src', 'model.rs'), 'utf8');
for (const f of MODEL_FILES) {
  const p = path.join(liteDir, f);
  const size = statSync(p).size;
  const actual = await sha256(p);
  const m = hooks.match(new RegExp(`IvyModelFile \\d+ "${f.replace('.', '\\.')}" (\\d+) \\d+ "([0-9A-Fa-f]+)"`));
  if (!m || Number(m[1]) !== size || m[2].toLowerCase() !== actual) {
    console.error(`hooks.nsh doesn't pin ${f} correctly: needs size ${size} and SHA-256 ${actual.toUpperCase()}.`);
    process.exit(1);
  }
  const r = modelRs.match(new RegExp(`\\("${f.replace('.', '\\.')}", ([\\d_]+), "([0-9a-f]+)"\\)`));
  if (!r || Number(r[1].replaceAll('_', '')) !== size || r[2] !== actual) {
    console.error(`src-tauri/src/model.rs doesn't pin ${f} correctly: needs size ${size} and SHA-256 ${actual}.`);
    process.exit(1);
  }
}

// hooks.nsh bundles Microsoft's VC++ redistributable (Ivy's speech engine needs its runtime). Not in git
// (24 MB): fetch the official one when missing, and refuse anything not signed by Microsoft.
const vcRedist = path.join(root, 'src-tauri', 'installer', 'vc_redist.x64.exe');
if (!existsSync(vcRedist)) {
  console.log('Downloading Microsoft VC++ redistributable...');
  execFileSync('curl.exe', ['-sL', '--fail', '-o', vcRedist, 'https://aka.ms/vs/17/release/vc_redist.x64.exe'], { stdio: 'inherit' });
}
const signer = execFileSync('powershell.exe', ['-NoProfile', '-Command',
  `$s = Get-AuthenticodeSignature '${vcRedist}'; if ($s.Status -eq 'Valid') { $s.SignerCertificate.Subject }`]).toString();
if (!signer.includes('O=Microsoft Corporation')) {
  console.error(`${vcRedist} is not validly signed by Microsoft; delete it and build again.`);
  process.exit(1);
}

// Ask cargo where it really builds: CARGO_TARGET_DIR or .cargo/config.toml can move target-dir.
const { target_directory } = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: path.join(root, 'src-tauri') }).toString(),
);
const nsisDir = path.join(target_directory, 'release', 'bundle', 'nsis');

console.log('Building installer (npm run tauri build)...');
execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['run', 'tauri', 'build'], {
  cwd: root,
  stdio: 'inherit',
  shell: true,
  env: { ...process.env, IVY_VC_REDIST: vcRedist }, // read by hooks.nsh at compile time
});

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
// Ivy's fine-tune adds the Commons Clause (not for sale) on top of Apache 2.0.
copyFileSync(path.join(root, 'LICENSES', 'IVY-MODEL-LICENSE.txt'), path.join(out, 'MODEL-LICENSE-Ivy.txt'));

console.log('Hashing (SHA256SUMS.txt)...');
const sums = [];
for (const f of readdirSync(out).filter((f) => f !== 'SHA256SUMS.txt')) {
  sums.push(`${await sha256(path.join(out, f))}  ${f}`);
}
writeFileSync(path.join(out, 'SHA256SUMS.txt'), sums.join('\n') + '\n');

console.log(`\nRelease folder ready (upload every file to the GitHub release):\n  ${out}`);
for (const f of readdirSync(out)) {
  console.log(`  ${f}  ${(statSync(path.join(out, f)).size / 1e9).toFixed(2)} GB`);
}
