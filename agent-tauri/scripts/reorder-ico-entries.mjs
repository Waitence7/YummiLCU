import { readFile, writeFile } from 'node:fs/promises';

const [inputPath] = process.argv.slice(2);
if (!inputPath) {
  throw new Error('usage: node reorder-ico-entries.mjs <icon.ico>');
}

const source = await readFile(inputPath);
const reserved = source.readUInt16LE(0);
const type = source.readUInt16LE(2);
const count = source.readUInt16LE(4);
if (reserved !== 0 || type !== 1 || count < 1) {
  throw new Error('invalid ICO header');
}

const entries = [];
for (let index = 0; index < count; index += 1) {
  const offset = 6 + index * 16;
  const width = source[offset] || 256;
  const height = source[offset + 1] || 256;
  const size = source.readUInt32LE(offset + 8);
  const imageOffset = source.readUInt32LE(offset + 12);
  entries.push({
    directory: Buffer.from(source.subarray(offset, offset + 16)),
    width,
    height,
    image: Buffer.from(source.subarray(imageOffset, imageOffset + size)),
  });
}

entries.sort((left, right) => right.width * right.height - left.width * left.height);

const headerSize = 6 + count * 16;
let imageOffset = headerSize;
for (const entry of entries) {
  entry.directory.writeUInt32LE(entry.image.length, 8);
  entry.directory.writeUInt32LE(imageOffset, 12);
  imageOffset += entry.image.length;
}

await writeFile(
  inputPath,
  Buffer.concat([
    source.subarray(0, 6),
    ...entries.map((entry) => entry.directory),
    ...entries.map((entry) => entry.image),
  ]),
);

console.log(
  `reordered ${count} ICO entries: ${entries.map(({ width, height }) => `${width}x${height}`).join(', ')}`,
);
