// 生成占位图标（品牌紫 #7c4dff），供 tauri-build 使用。
// 用法: node scripts/gen_icon.mjs
import { deflateSync } from 'node:zlib';
import { writeFileSync, mkdirSync } from 'node:fs';

const SIZE = 256;

// CRC32 (PNG 需要)
const table = new Int32Array(256);
for (let n = 0; n < 256; n++) {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  table[n] = c;
}
function crc32(buf) {
  let crc = -1;
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ table[(crc ^ buf[i]) & 0xff];
  return (crc ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const t = Buffer.from(type, 'ascii');
  const c = Buffer.alloc(4);
  c.writeUInt32BE(crc32(Buffer.concat([t, data])));
  return Buffer.concat([len, t, data, c]);
}

// 每行前加 filter byte 0，RGBA 纯紫
const stride = SIZE * 4 + 1;
const raw = Buffer.alloc(SIZE * stride);
for (let y = 0; y < SIZE; y++) {
  const row = y * stride;
  raw[row] = 0;
  for (let x = 0; x < SIZE; x++) {
    const o = row + 1 + x * 4;
    raw[o] = 0x7c;
    raw[o + 1] = 0x4d;
    raw[o + 2] = 0xff;
    raw[o + 3] = 0xff;
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
ihdr[10] = 0;
ihdr[11] = 0;
ihdr[12] = 0;

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]);

// ICO：header + 1 个 dir entry + PNG 数据（Vista+ 支持 PNG 压缩 ICO）
const icoHeader = Buffer.alloc(6);
icoHeader.writeUInt16LE(0, 0);
icoHeader.writeUInt16LE(1, 2);
icoHeader.writeUInt16LE(1, 4);
const dir = Buffer.alloc(16);
dir[0] = 0; // 256 -> 0
dir[1] = 0;
dir[2] = 0;
dir[3] = 0;
dir.writeUInt16LE(1, 4);
dir.writeUInt16LE(32, 6);
dir.writeUInt32LE(png.length, 8);
dir.writeUInt32LE(22, 12);
const ico = Buffer.concat([icoHeader, dir, png]);

const outDir = new URL('../apps/desktop/src-tauri/icons/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
mkdirSync(outDir, { recursive: true });
writeFileSync(outDir + 'icon.png', png);
writeFileSync(outDir + 'icon.ico', ico);
console.log('icon.png', png.length, 'bytes');
console.log('icon.ico', ico.length, 'bytes');
