#!/usr/bin/env node
// One-time model acquisition for local dev/build — never run by the shipped
// app itself (see IVY.md: "Network | none, ever"). Downloads real Whisper
// Voxtral Mini 3B 2507 (end-to-end multimodal STT + cleanup) and Whisper
// large-v3-turbo (fallback STT) weights into src-tauri/models/, which
// tauri.conf.json bundles into the installer as a resource directory so the
// packaged app makes zero network calls at runtime.
//
// Whisper large-v3-turbo is retained as a fallback engine.
//
// Voxtral Mini 3B 2507 base model (~2.36GB Q4_K_M) + mmproj (~0.68GB Q8_0)
// are downloaded from ggml-org/Voxtral-Mini-3B-2507-GGUF.
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
    url: 'https://huggingface.co/ggml-org/Voxtral-Mini-3B-2507-GGUF/resolve/main/Voxtral-Mini-3B-2507-Q4_K_M.gguf',
    dest: path.join(modelsDir, 'voxtral-ivy', 'Voxtral-Mini-3B-2507-Q4_K_M.gguf'),
  },
  {
    url: 'https://huggingface.co/ggml-org/Voxtral-Mini-3B-2507-GGUF/resolve/main/mmproj-Voxtral-Mini-3B-2507-Q8_0.gguf',
    dest: path.join(modelsDir, 'voxtral-ivy', 'mmproj-Voxtral-Mini-3B-2507-Q8_0.gguf'),
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
