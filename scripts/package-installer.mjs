#!/usr/bin/env node
// Builds the real NSIS installer, then folds it and the cleanup model's
// GGUF (~2GB — too big to embed under NSIS's 2GB payload cap alongside
// Whisper, see src-tauri/installer/hooks.nsh) into a single downloadable exe, instead of
// shipping two files side by side. One file on a GitHub release page is a
// lot less confusing for someone downloading Ivy than "download both of
// these and keep them together."
//
// How: compile a small outer NSIS wrapper (src-tauri/installer/wrapper.nsi)
// that embeds the real installer normally (under NSIS's cap),
// then append the GGUF's raw bytes onto the *end* of that compiled exe,
// followed by a 16-byte footer (8-byte magic + 8-byte little-endian GGUF
// length). NSIS's cap only applies to its own internal data blocks, not to
// the total file size on disk, so the appended tail is invisible to it —
// same trick as hooks.nsh's "companion file, not embedded", just glued
// onto one file instead of shipped as two. At install time the wrapper
// reads its own footer, carves the GGUF back out into a temp folder next
// to the real installer, and launches it exactly as if both files had
// been downloaded together.
import { copyFileSync, existsSync, readdirSync, statSync, openSync, readSync, writeSync, closeSync, unlinkSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const gguf = path.join(root, 'src-tauri', 'models', 'voxtral-ivy', 'Voxtral-Mini-3B-2507-Q4_K_M.gguf');
// Ask cargo where it really builds: src-tauri/.cargo/config.toml can move
// target-dir, and a hardcoded src-tauri/target path silently found nothing.
const { target_directory } = JSON.parse(
  execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], { cwd: path.join(root, 'src-tauri') }).toString(),
);
const nsisDir = path.join(target_directory, 'release', 'bundle', 'nsis');
const wrapperNsi = path.join(root, 'src-tauri', 'installer', 'wrapper.nsi');
const makensis = path.join(process.env.LOCALAPPDATA ?? '', 'tauri', 'NSIS', 'makensis.exe');

// Must match wrapper.nsi's `!define FOOTER_MAGIC` exactly.
const FOOTER_MAGIC = 0x4956594658585901n;

if (!existsSync(gguf)) {
  console.error(`Missing ${gguf} — run \`npm run setup-models\` first.`);
  process.exit(1);
}
if (!existsSync(makensis)) {
  console.error(`Missing ${makensis} — run \`npm run tauri build\` once first so Tauri provisions its NSIS toolchain.`);
  process.exit(1);
}

console.log('Building installer (npm run tauri build)...');
const npmCmd = process.platform === 'win32' ? 'npm.cmd' : 'npm';
execFileSync(npmCmd, ['run', 'tauri', 'build'], { cwd: root, stdio: 'inherit', shell: true });

const installerName = readdirSync(nsisDir).find((f) => f.endsWith('-setup.exe'));
if (!installerName) {
  console.error(`No installer found in ${nsisDir}`);
  process.exit(1);
}
const installerPath = path.join(nsisDir, installerName);

console.log('Compiling single-file wrapper...');
const wrapperOut = path.join(root, 'src-tauri', 'installer', 'wrapper-unstamped.exe');
execFileSync(makensis, [`/DINSTALLER_EXE=${installerPath}`, wrapperNsi], {
  cwd: path.dirname(wrapperNsi),
  stdio: 'inherit',
});
if (!existsSync(wrapperOut)) {
  console.error(`makensis did not produce ${wrapperOut}`);
  process.exit(1);
}

console.log('Appending model data...');
// A genuinely different base name, not a case-only rename of
// installerName — Windows/NTFS paths are case-insensitive, so
// "-setup.exe" vs "-Setup.exe" is the same file on disk and writing one
// truncates the other out from under it.
const combinedPath = path.join(nsisDir, 'Ivy_Setup.exe');

const wrapperFd = openSync(wrapperOut, 'r');
const outFd = openSync(combinedPath, 'w');
try {
  // 1) The compiled wrapper (real installer embedded inside it).
  const chunk = Buffer.alloc(8 * 1024 * 1024);
  let bytes;
  while ((bytes = readSync(wrapperFd, chunk, 0, chunk.length, null)) > 0) {
    writeSync(outFd, chunk, 0, bytes);
  }
  // 2) The GGUF, raw.
  const ggufFd = openSync(gguf, 'r');
  let ggufBytes = 0n;
  try {
    while ((bytes = readSync(ggufFd, chunk, 0, chunk.length, null)) > 0) {
      writeSync(outFd, chunk, 0, bytes);
      ggufBytes += BigInt(bytes);
    }
  } finally {
    closeSync(ggufFd);
  }
  // 3) Footer: magic then length, both little-endian uint64 — must match
  //    the read order in wrapper.nsi (`*$Buf(l .r0, l .r1)`).
  const footer = Buffer.alloc(16);
  footer.writeBigUInt64LE(FOOTER_MAGIC, 0);
  footer.writeBigUInt64LE(ggufBytes, 8);
  writeSync(outFd, footer);
} finally {
  closeSync(wrapperFd);
  closeSync(outFd);
}
unlinkSync(wrapperOut);
// The plain installer and standalone GGUF were only ever intermediates for
// this step — remove them so the release folder has exactly one file in
// it, matching what actually gets published.
unlinkSync(installerPath);

const finalSize = statSync(combinedPath).size;
console.log(`\nReady to distribute — one file, ${(finalSize / 1e9).toFixed(2)}GB:`);
console.log(`  ${combinedPath}`);
