#!/usr/bin/env node
// One-time model download for building Ivy from source — never run by the shipped app itself
// (IVY.md: "Network | none, ever"). Fetches Ivy's lite model (Qwen3-ASR-1.7B fine-tuned by the Ivy lab,
// Apache-2.0, as GGUF) from the GitHub release into src-tauri/models/ivy-lite/, where lite.rs loads it in
// dev builds and scripts/package-installer.mjs picks it up for the release folder.
// Each file must match its SHA-256 (the release's SHA256SUMS.txt): a tampered GGUF can attack llama.cpp.
import { createReadStream, createWriteStream, existsSync, mkdirSync, statSync, unlinkSync } from 'node:fs';
import { rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const RELEASE = 'https://github.com/raj-7676/IVY-/releases/download/v0.1.4';
const SHA256 = {
  'ivy-lite-Q8_0.gguf': 'da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed',
  'mmproj-ivy-lite-f16.gguf': '07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00',
};
const root = path.dirname(fileURLToPath(import.meta.url));
const liteDir = path.join(root, '..', 'src-tauri', 'models', 'ivy-lite');

async function sha256(file) {
  const h = createHash('sha256');
  for await (const chunk of createReadStream(file)) h.update(chunk);
  return h.digest('hex');
}

for (const [name, expected] of Object.entries(SHA256)) {
  const dest = path.join(liteDir, name);
  mkdirSync(liteDir, { recursive: true });
  if (existsSync(dest) && statSync(dest).size > 0) {
    if ((await sha256(dest)) !== expected) throw new Error(`${dest} doesn't match its SHA-256. Delete it and run again.`);
    console.log(`skip (already present, hash ok): ${name}`);
    continue;
  }
  const url = `${RELEASE}/${name}`;
  console.log(`downloading ${url}`);
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok || !res.body) throw new Error(`${url} -> HTTP ${res.status}`);
  await pipeline(Readable.fromWeb(res.body), createWriteStream(`${dest}.tmp`));
  if ((await sha256(`${dest}.tmp`)) !== expected) {
    unlinkSync(`${dest}.tmp`);
    throw new Error(`${name} downloaded but doesn't match its SHA-256; deleted it. Try again.`);
  }
  await rename(`${dest}.tmp`, dest);
  console.log(`done (hash ok): ${name}`);
}
console.log('Lite model present in src-tauri/models/ivy-lite/.');
