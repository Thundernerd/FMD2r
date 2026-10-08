// Starts `fmd2r serve` on a temp data dir holding one fixture module, plus a local "website"
// serving its page images, for the real-server smoke tests (playwright.real.config.ts).
// Nothing reaches the network: the module's pages come from 127.0.0.1.
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const APP_PORT = 4180;
const SITE_PORT = 4181;
const site = `http://127.0.0.1:${SITE_PORT}`;

// A 1x1 PNG.
const PNG = Buffer.from(
	'89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c494441' +
		'54789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082',
	'hex'
);

const dir = mkdtempSync(join(tmpdir(), 'fmd2r-e2e-'));
// `serve` loads the modules from <data dir>/lua.
mkdirSync(join(dir, 'data/lua/modules'), { recursive: true });
// Chapter `/c/<n>` has 3 pages.
writeFileSync(
	join(dir, 'data/lua/modules/Fixture.lua'),
	`function Init() local m = NewWebsiteModule(); m.ID='fixture'; m.Name='Fixture'; m.RootURL='${site}'; m.OnGetPageNumber='GPN' end
function GPN()
  for i = 1, 3 do TASK.PageLinks.Add('${site}/img' .. URL .. '/' .. i) end
  return true
end
`
);

let ready = false;
// Images come slowly enough for the page to show a task downloading.
http
	.createServer((req, res) => {
		if (req.url === '/ready') {
			res.writeHead(ready ? 200 : 503).end();
		} else if (req.url?.startsWith('/img/')) {
			setTimeout(() => res.writeHead(200, { 'content-type': 'image/png' }).end(PNG), 300);
		} else {
			res.writeHead(404).end();
		}
	})
	.listen(SITE_PORT, '127.0.0.1');

const root = join(dirname(fileURLToPath(import.meta.url)), '../..');
const server = spawn(
	'cargo',
	[
		'run',
		'--quiet',
		'-p',
		'fmd2r',
		'--',
		'serve',
		'--bind',
		`127.0.0.1:${APP_PORT}`,
		'--data-dir',
		join(dir, 'data'),
		'--no-module-updates'
	],
	{ cwd: root, stdio: 'inherit' }
);

const stop = () => {
	server.kill('SIGTERM');
	rmSync(dir, { recursive: true, force: true });
	process.exit(0);
};
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
server.on('exit', (code) => {
	console.error(`fmd2r serve exited with ${code}`);
	process.exit(1);
});

// Once the server answers: pack chapters as CBZ into the temp dir, then report ready.
const app = `http://127.0.0.1:${APP_PORT}`;
for (;;) {
	try {
		const res = await fetch(`${app}/api/settings`, {
			method: 'PATCH',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({
				output: { format: 'cbz' },
				saveto: { default_dir: join(dir, 'out') }
			})
		});
		if (res.ok) break;
	} catch {
		// Not listening yet.
	}
	await new Promise((resolve) => setTimeout(resolve, 500));
}
ready = true;
