#!/usr/bin/env node
// One-time model acquisition for local dev/build — never run by the shipped
// app itself (see IVY.md: "Network | none, ever"). Downloads real Whisper
// large-v3-turbo (STT, int8 ONNX) and Qwen 2.5 3B (cleanup & summarization)
// weights into src-tauri/models/, which tauri.conf.json bundles into the
// installer as a resource directory so the packaged app makes zero network
// calls at runtime.
//
// Whisper large-v3-turbo replaces Moonshine v2 base as of 2026-09-24 —
// multilingual (99 languages, including Hindi for Indian-English loanwords),
// real lower WER on published benchmarks. int8 quantized encoder+decoder
// (~1.08GB total) chosen for CPU speed; see IVY.md for the real benchmark
// sources this decision was made from.
//
// Qwen 2.5 3B's GGUF (~1.96GB) is downloaded here for real (full Q4_K_M
// quality). 1.5B was dropped 2026-09-27 (Yash's call: context-understanding
// quality over install size for this feature).
import { createWriteStream, existsSync, mkdirSync, statSync } from 'node:fs';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const modelsDir = path.join(root, '..', 'src-tauri', 'models');

const FILES = [
  {
    url: 'https://huggingface.co/onnx-community/whisper-large-v3-turbo-ONNX/resolve/main/onnx/encoder_model_int8.onnx',
    dest: path.join(modelsDir, 'whisper', 'encoder_model_int8.onnx'),
  },
  {
    url: 'https://huggingface.co/onnx-community/whisper-large-v3-turbo-ONNX/resolve/main/onnx/decoder_model_merged_int8.onnx',
    dest: path.join(modelsDir, 'whisper', 'decoder_model_merged_int8.onnx'),
  },
  {
    url: 'https://huggingface.co/onnx-community/whisper-large-v3-turbo-ONNX/resolve/main/tokenizer.json',
    dest: path.join(modelsDir, 'whisper', 'tokenizer.json'),
  },
  {
    url: 'https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf',
    dest: path.join(modelsDir, 'qwen2.5-3b', 'qwen2.5-3b-instruct-q4_k_m.gguf'),
  },
];

async function download({ url, dest }) {
  mkdirSync(path.dirname(dest), { recursive: true });
  if (existsSync(dest) && statSync(dest).size > 0) {
    console.log(`skip (already present): ${path.relative(root, dest)}`);
    return;
  }
  console.log(`downloading ${url}`);
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok || !res.body) throw new Error(`${url} -> HTTP ${res.status}`);
  const tmp = `${dest}.tmp`;
  await pipeline(Readable.fromWeb(res.body), createWriteStream(tmp));
  const fs = await import('node:fs/promises');
  await fs.rename(tmp, dest);
  console.log(`done: ${path.relative(root, dest)}`);
}

for (const file of FILES) {
  await download(file);
}
console.log('All models present in src-tauri/models/.');
