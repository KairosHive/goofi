import { mkdir, copyFile } from 'node:fs/promises';
import path from 'node:path';

const out = process.env.GOOFI_PLUGIN_OUT_DIR;
await mkdir(out, { recursive: true });
await copyFile('index.js', path.join(out, 'index.js'));
