// Starts `fmd2r serve` on a temp data dir holding one fixture module, plus a local "website"
// the module reads from, for the end-to-end tests (playwright.real.config.ts). Nothing reaches
// the network: the module's covers and pages come from 127.0.0.1.
//
// The site:
// - `/manga/<key>` is a series titled `Fixture <key>` with a cover and chapters
//   `/c/<key>/1` to `/c/<key>/3`, each of PAGES pages; `POST /publish/<key>` adds a chapter;
// - page `n` is a PNG `n` pixels wide, so a test can tell the pages apart;
// - chapter 2's pages are held until `POST /release/<key>`, so a test can catch a task mid-download;
// - `GET /fetches/<key>/<chapter>` counts the page requests for that chapter;
// - `PUT /custom-css` writes its body to the data dir's `custom.css`, `DELETE /custom-css` removes it;
// - `POST /restart` stops the server (SIGTERM) and starts it again on the same data dir, and
//   answers once it is back.
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync } from 'node:zlib';

const APP_PORT = 4180;
const SITE_PORT = 4181;
const site = `http://127.0.0.1:${SITE_PORT}`;
const app = `http://127.0.0.1:${APP_PORT}`;
/** Pages per chapter; past 9 so a lexical sort of unpadded names would get them wrong. */
const PAGES = 12;

/**
 * A grayscale PNG `width` pixels wide and 1 high.
 * @param {number} width
 */
function png(width) {
	/** @type {(type: string, data: Buffer) => Buffer} */
	const chunk = (type, data) => {
		const len = Buffer.alloc(4);
		len.writeUInt32BE(data.length);
		const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const crc = Buffer.alloc(4);
		crc.writeUInt32BE(crc32(body));
		return Buffer.concat([len, body, crc]);
	};
	const ihdr = Buffer.alloc(13);
	ihdr.writeUInt32BE(width, 0);
	ihdr.writeUInt32BE(1, 4);
	ihdr[8] = 8; // bit depth
	ihdr[9] = 0; // grayscale
	// One scanline: filter byte 0, then the pixels.
	const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(width, 0x80)]);
	return Buffer.concat([
		Buffer.from('89504e470d0a1a0a', 'hex'),
		chunk('IHDR', ihdr),
		chunk('IDAT', deflateSync(row)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

const dir = mkdtempSync(join(tmpdir(), 'fmd2r-e2e-'));
// `serve` loads the modules from <data dir>/lua.
mkdirSync(join(dir, 'data/lua/modules'), { recursive: true });
writeFileSync(
	join(dir, 'data/lua/modules/Fixture.lua'),
	`function Init()
  local m = NewWebsiteModule()
  m.ID = 'fixture'; m.Name = 'Fixture'; m.RootURL = '${site}'
  m.OnGetInfo = 'GetInfo'; m.OnGetPageNumber = 'GPN'
end
function GetInfo()
  local key = URL:match('^/manga/(.+)$')
  if not key then return no_error end
  -- The site answers with the number of chapters.
  if not HTTP.GET(MANGAINFO.URL) then return net_problem end
  local chapters = tonumber(HTTP.Document.ToString())
  MANGAINFO.Title = 'Fixture ' .. key
  MANGAINFO.CoverLink = MODULE.RootURL .. '/cover.png'
  MANGAINFO.Status = '1'
  for i = 1, chapters do
    MANGAINFO.ChapterLinks.Add('/c/' .. key .. '/' .. i)
    MANGAINFO.ChapterNames.Add('Ch. ' .. i)
  end
  return no_error
end
function GPN()
  for i = 1, ${PAGES} do TASK.PageLinks.Add(MODULE.RootURL .. '/img' .. URL .. '/' .. i) end
  return true
end
`
);

/** Chapters a series lists until `/publish` adds one. */
const CHAPTERS = 3;

let ready = false;
/** Chapters per series key, once `/publish` changed them. */
const published = new Map();
/** Page requests per `<key>/<chapter>`. */
const fetches = new Map();
/** Series keys whose chapter 2 is no longer held. */
const released = new Set();
/** Answers for held chapter 2 pages, by series key. */
const held = new Map();

http
	.createServer((req, res) => {
		const url = req.url ?? '';
		const image = url.match(/^\/img\/c\/([^/]+)\/(\d+)\/(\d+)$/);
		const release = url.match(/^\/release\/([^/]+)$/);
		const series = url.match(/^\/manga\/([^/]+)$/);
		const publish = url.match(/^\/publish\/([^/]+)$/);
		const fetched = url.match(/^\/fetches\/([^/]+\/\d+)$/);
		if (url === '/ready') {
			res.writeHead(ready ? 200 : 503).end();
		} else if (series) {
			res
				.writeHead(200, { 'content-type': 'text/plain' })
				.end(String(published.get(series[1]) ?? CHAPTERS));
		} else if (req.method === 'POST' && publish) {
			published.set(publish[1], (published.get(publish[1]) ?? CHAPTERS) + 1);
			res.writeHead(204).end();
		} else if (fetched) {
			res
				.writeHead(200, { 'content-type': 'text/plain' })
				.end(String(fetches.get(fetched[1]) ?? 0));
		} else if (url === '/cover.png') {
			res.writeHead(200, { 'content-type': 'image/png' }).end(png(4));
		} else if (image) {
			const [, key, chapter, page] = image;
			fetches.set(`${key}/${chapter}`, (fetches.get(`${key}/${chapter}`) ?? 0) + 1);
			const answer = () => {
				// A request from a server stopped since has nobody to answer.
				if (!res.destroyed) {
					res.writeHead(200, { 'content-type': 'image/png' }).end(png(Number(page)));
				}
			};
			if (chapter === '2' && !released.has(key)) {
				held.set(key, [...(held.get(key) ?? []), answer]);
			} else {
				// Slow enough for the queue to show a task downloading.
				setTimeout(answer, 50);
			}
		} else if (req.method === 'POST' && release) {
			const key = release[1];
			released.add(key);
			for (const answer of held.get(key) ?? []) answer();
			held.delete(key);
			res.writeHead(204).end();
		} else if (url === '/custom-css' && (req.method === 'PUT' || req.method === 'DELETE')) {
			const file = join(dir, 'data/custom.css');
			if (req.method === 'DELETE') {
				rmSync(file, { force: true });
				res.writeHead(204).end();
			} else {
				/** @type {Buffer[]} */
				const chunks = [];
				req.on('data', (chunk) => chunks.push(chunk));
				req.on('end', () => {
					writeFileSync(file, Buffer.concat(chunks));
					res.writeHead(204).end();
				});
			}
		} else if (req.method === 'POST' && url === '/restart') {
			restart().then(
				() => res.writeHead(204).end(),
				(e) => res.writeHead(500).end(String(e))
			);
		} else {
			res.writeHead(404).end();
		}
	})
	.listen(SITE_PORT, '127.0.0.1');

const root = join(dirname(fileURLToPath(import.meta.url)), '../..');
const build = spawnSync('cargo', ['build', '--quiet', '-p', 'fmd2r'], {
	cwd: root,
	stdio: 'inherit'
});
if (build.status !== 0) process.exit(1);
const binary = join(resolve(root, process.env.CARGO_TARGET_DIR ?? 'target'), 'debug/fmd2r');

/**
 * The running server; its exit fails the run unless `stopServer` asked for it.
 * @type {import('node:child_process').ChildProcess}
 */
let server;
let stopping = false;

function startServer() {
	server = spawn(
		binary,
		[
			'serve',
			'--bind',
			`127.0.0.1:${APP_PORT}`,
			'--data-dir',
			join(dir, 'data'),
			'--no-module-updates'
		],
		{ cwd: root, stdio: 'inherit' }
	);
	server.on('exit', (code, signal) => {
		if (stopping) return;
		console.error(`fmd2r serve exited with ${code ?? signal}`);
		process.exit(1);
	});
}

/** Stops the server as `docker stop` would: SIGTERM, then SIGKILL if it takes too long. */
async function stopServer() {
	stopping = true;
	const exited = new Promise((done) =>
		server.exitCode === null && server.signalCode === null ? server.once('exit', done) : done(null)
	);
	server.kill('SIGTERM');
	const kill = setTimeout(() => server.kill('SIGKILL'), 15_000);
	await exited;
	clearTimeout(kill);
	stopping = false;
}

/**
 * Waits until `request` (retried every 500 ms while the server is not listening) succeeds.
 * @param {() => Promise<Response>} request
 */
async function untilUp(request) {
	for (;;) {
		try {
			const res = await request();
			if (res.ok) return;
		} catch {
			// Not listening yet.
		}
		await new Promise((done) => setTimeout(done, 500));
	}
}

async function restart() {
	await stopServer();
	startServer();
	await untilUp(() => fetch(`${app}/api/settings`));
}

const stop = async () => {
	await stopServer();
	rmSync(dir, { recursive: true, force: true });
	process.exit(0);
};
process.on('SIGTERM', stop);
process.on('SIGINT', stop);

startServer();
// Once the server answers: pack chapters as CBZ into the temp dir, mark setup done so the flow
// starts on the library rather than the fresh install's setup wizard, then report ready.
await untilUp(() =>
	fetch(`${app}/api/settings`, {
		method: 'PATCH',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({
			general: { setup_completed: true },
			output: { format: 'cbz' },
			saveto: { default_dir: join(dir, 'out') }
		})
	})
);
ready = true;
