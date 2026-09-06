#!/usr/bin/env node
/**
 * Pack resized PNGs into real .ico (Windows) and .icns (macOS) containers.
 *
 * No system tools or npm deps: both formats embed PNG blobs with tiny
 * headers, so plain Node Buffer writes are enough. ICO is only consumed by
 * the Windows bundler, ICNS only by the macOS bundler; Linux builds use the
 * PNGs directly. Run after regenerating icons/icon.png:
 *
 *   node scripts/build-icons.mjs
 *
 * Expects magick (ImageMagick 7) for resizing.
 */
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ICONS = path.resolve(__dirname, '../src-tauri/icons');
mkdirSync(ICONS, { recursive: true });

const resize = (size, outName) => {
  const out = path.join(ICONS, outName);
  execFileSync('magick', [
    path.join(ICONS, 'icon.png'),
    '-resize', `${size}x${size}`,
    '-background', 'none',
    '-gravity', 'center',
    '-extent', `${size}x${size}`,
    '-type', 'TrueColorAlpha',
    '-define', 'png:color-type=6',
    out,
  ]);
  return readFileSync(out);
};

// ---- ICO: ICONDIR + ICONDIRENTRY[] + PNG blobs ----
const buildIco = (entries) => {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + dir.length;
  entries.forEach(({ size, png }, i) => {
    const o = i * 16;
    dir.writeUInt8(size >= 256 ? 0 : size, o + 0);
    dir.writeUInt8(size >= 256 ? 0 : size, o + 1);
    dir.writeUInt8(0, o + 2); // palette
    dir.writeUInt8(0, o + 3); // reserved
    dir.writeUInt16LE(1, o + 4); // planes
    dir.writeUInt16LE(32, o + 6); // bit count
    dir.writeUInt32LE(png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
};

// ---- ICNS: 'icns' magic + (OSType, u32 length, payload)[] ----
const buildIcns = (elements) => {
  const parts = elements.map(({ type, png }) => {
    const head = Buffer.alloc(8);
    head.write(type, 0, 4, 'ascii');
    head.writeUInt32BE(8 + png.length, 4);
    return Buffer.concat([head, png]);
  });
  const head = Buffer.alloc(8);
  head.write('icns', 0, 4, 'ascii');
  head.writeUInt32BE(8 + parts.reduce((n, p) => n + p.length, 0), 4);
  return Buffer.concat([head, ...parts]);
};

const png16 = resize(16, 'tmp-16.png');
const png32 = resize(32, '32x32.png');
const png48 = resize(48, 'tmp-48.png');
const png64 = resize(64, 'tmp-64.png');
const png128 = resize(128, '128x128.png');
const png256 = resize(256, '128x128@2x.png');
const png512 = resize(512, 'tmp-512.png');
const png1024 = readFileSync(path.join(ICONS, 'icon.png'));

writeFileSync(
  path.join(ICONS, 'icon.ico'),
  buildIco([
    { size: 16, png: png16 },
    { size: 32, png: png32 },
    { size: 48, png: png48 },
    { size: 64, png: png64 },
    { size: 128, png: png128 },
    { size: 256, png: png256 },
  ]),
);

// Mirrors iconutil output: ic11=32 ic12=64 ic07=128 ic08=256 ic13=256 ic14=128
// ic09=512 ic10=1024 (PNG payloads are fine on macOS 11+, our minimum).
writeFileSync(
  path.join(ICONS, 'icon.icns'),
  buildIcns([
    { type: 'ic11', png: png32 },
    { type: 'ic12', png: png64 },
    { type: 'ic07', png: png128 },
    { type: 'ic14', png: png128 },
    { type: 'ic08', png: png256 },
    { type: 'ic13', png: png256 },
    { type: 'ic09', png: png512 },
    { type: 'ic10', png: png1024 },
  ]),
);

for (const tmp of ['tmp-16.png', 'tmp-48.png', 'tmp-64.png', 'tmp-512.png']) {
  try {
    const { unlinkSync } = await import('node:fs');
    unlinkSync(path.join(ICONS, tmp));
  } catch { /* already gone */ }
}

console.log('icons written:', ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.ico', 'icon.icns'].join(', '));
