import { crc32, inflateRawSync } from 'node:zlib';

/** One file in a zip archive. */
export interface ZipEntry {
	name: string;
	data: Buffer;
}

/**
 * The files of the zip archive `zip`, in central directory order. Reads stored and deflated
 * entries, which is all the server writes; throws on anything else, or on a CRC mismatch.
 */
export function unzip(zip: Buffer): ZipEntry[] {
	// The end of central directory record: its signature, searched from the end past any comment.
	const eocd = zip.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
	if (eocd < 0) throw new Error('not a zip archive');
	const count = zip.readUInt16LE(eocd + 10);
	let at = zip.readUInt32LE(eocd + 16);
	const entries: ZipEntry[] = [];
	for (let i = 0; i < count; i++) {
		if (zip.readUInt32LE(at) !== 0x02014b50) throw new Error(`bad central directory entry ${i}`);
		const method = zip.readUInt16LE(at + 10);
		const crc = zip.readUInt32LE(at + 16);
		const size = zip.readUInt32LE(at + 20);
		const nameLength = zip.readUInt16LE(at + 28);
		const extraLength = zip.readUInt16LE(at + 30);
		const commentLength = zip.readUInt16LE(at + 32);
		const local = zip.readUInt32LE(at + 42);
		const name = zip.toString('utf8', at + 46, at + 46 + nameLength);
		at += 46 + nameLength + extraLength + commentLength;

		if (zip.readUInt32LE(local) !== 0x04034b50) throw new Error(`bad local header for ${name}`);
		const start = local + 30 + zip.readUInt16LE(local + 26) + zip.readUInt16LE(local + 28);
		const raw = zip.subarray(start, start + size);
		const data = method === 0 ? Buffer.from(raw) : method === 8 ? inflateRawSync(raw) : undefined;
		if (!data) throw new Error(`${name}: unsupported compression method ${method}`);
		if (crc32(data) !== crc) throw new Error(`${name}: CRC mismatch`);
		entries.push({ name, data });
	}
	return entries;
}
