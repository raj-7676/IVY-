#!/usr/bin/env node
// One-time model download for building Ivy from source — never run by the shipped app itself
// (IVY.md: "Network | none, ever"). Fetches Ivy's lite model (Qwen3-ASR-1.7B fine-tuned by the Ivy lab,
// Apache-2.0, as GGUF) from the GitHub release into src-tauri/models/ivy-lite/, where lite.rs loads it in
// dev builds and scripts/package-installer.mjs picks it up for the release folder.
import { createWriteStream, existsSync, mkdirSync, statSync } from 'node:fs';
import { rename } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const RELEASE = 'https://github.com/raj-7676/IVY-/releases/download/v0.1.4';
const root = path.dirname(fileURLToPath(import.meta.url));
const liteDir = path.join(root, '..', 'src-tauri', 'models', 'ivy-lite');

for (const name of ['ivy-lite-Q8_0.gguf', 'mmproj-ivy-lite-f16.gguf']) {
  const dest = path.join(liteDir, name);
  mkdirSync(liteDir, { recursive: true });
  if (existsSync(dest) && statSync(dest).size > 0) {
    console.log(`skip (already present): ${name}`);
    continue;
  }
  const url = `${RELEASE}/${name}`;
  console.log(`downloading ${url}`);
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok || !res.body) throw new Error(`${url} -> HTTP ${res.status}`);
  await pipeline(Readable.fromWeb(res.body), createWriteStream(`${dest}.tmp`));
  await rename(`${dest}.tmp`, dest);
  console.log(`done: ${name}`);
}
console.log('Lite model present in src-tauri/models/ivy-lite/.');
