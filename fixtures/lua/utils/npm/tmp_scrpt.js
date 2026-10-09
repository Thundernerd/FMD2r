        const puppeteer = require('puppeteer');
        const http = require('http');
        const fs = require('fs');

        (async () => {
            const url = "https://comix.to/title/02l05-after-a-meal/7033231-chapter-1";
            const fsHost = "localhost";
            const fsPort = 8191;
            const useFS = false;

            let currentCookies = [];
            let currentUA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";

            const fsRequest = () => new Promise(resolve => {
                const payload = JSON.stringify({ cmd: 'request.get', url: url, maxTimeout: 60000 });
                const req = http.request({
                    hostname: fsHost,
                    port: fsPort,
                    path: '/v1',
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(payload) }
                }, res => {
                    let body = '';
                    res.on('data', d => body += d);
                    res.on('end', () => {
                        try { resolve(JSON.parse(body)); } catch(e) { resolve(null); }
                    });
                });
                req.on('error', () => resolve(null));
                req.write(payload);
                req.end();
            });

            let browser;
            try {
                browser = await puppeteer.launch({
                    headless: true,
                    args: [
                        '--disable-extensions', // Prevent browser extensions
                        '--disable-webgl', // Disable WebGL
                        '--disable-webrtc', // Disable WebRTC
                        '--disable-background-networking', // Prevents background requests
						'--disable-blink-features=AutomationControlled' // Prevents detection of automation
                    ]
                });
                const page = await browser.newPage();
                await page.setBypassCSP(false); // Enforce CSP

                if (currentUA) {
                    await page.setUserAgent(currentUA);
                }
                if (currentCookies && currentCookies.length > 0) {
                    await page.setCookie(...currentCookies);
                }

                let response;
                try {
                    response = await page.goto(url, { waitUntil: "domcontentloaded", timeout: 90000 });
                } catch (navError) {
                    console.log("Navigation error:", navError.message);
                }

                let status = response ? response.status() : 200;
                let title = await page.title();
                
                // Determine if active FMD2 credentials hit a wall
                let isCloudflare = [503, 403, 429].includes(status) || /Just a moment|Attention Required|Cloudflare/i.test(title);

                if (isCloudflare && useFS) {
                    const fsData = await fsRequest();
                    
                    if (fsData && fsData.status === 'ok') {
                        currentUA = fsData.solution.userAgent;
                        currentCookies = fsData.solution.cookies;

                        if (currentUA) await page.setUserAgent(currentUA);
                        if (currentCookies && currentCookies.length > 0) {
                            await page.setCookie(...currentCookies);
                        }

                        try {
                            await page.goto(url, { waitUntil: "domcontentloaded", timeout: 90000 });
                        } catch (navError) {
                            console.log("Re-navigation error:", navError.message);
                        }
                    } else {
                        console.log("FlareSolverr failed or unreachable. Proceeding with existing structure...");
                    }
                }

                	const resultJSON = await page.evaluate(async () => {
		return new Promise((resolve) => {
			const originalParse = JSON.parse;
			let submitted = false;
			
			const submit = (data) => {
				if (submitted) return;
				submitted = true;
				resolve(data);
			};

			JSON.parse = new Proxy(originalParse, {
				apply(target, thisArg, args) {
					const parsed = Reflect.apply(target, thisArg, args);
					try {
								if (parsed && parsed.result && parsed.result.pages) {
			const res = parsed.result;
			const links = [];
			const pages = res.pages;
			const items = pages.items || (Array.isArray(pages) ? pages : []);
			let base = (pages.baseUrl || '').replace(/\/$/, '');

			for (let i = 0; i < items.length; i++) {
				let item = items[i];
				let url = typeof item === 'string' ? item : item.url;
				if (!url) continue;
				let full = url.startsWith('http') ? url : base + '/' + url.replace(/^\//, '');
				
				let isV3 = (item.s === 1) || full.includes('?v3') || full.includes('&v3');
				let isLegacy = !isV3 && ((i + 1) % 4 === 0);
				
				if (isV3) {
					if (!full.includes('v3')) {
						full += (full.includes('?') ? '&' : '?') + 'v3';
					}
				} else if (isLegacy) {
					full += '#scrambled';
				}
				links.push(full);
			}
			submit({ links: links });
		}
						} catch (e) {}
					return parsed;
				}
			});

			setTimeout(() => submit({ error: 'Timed out waiting for data' }), 60000);
		});
	});
	console.log(JSON.stringify(resultJSON));
	
                const finalCookies = await page.cookies();
                const finalUA = currentUA || (await browser.userAgent());
                
                fs.writeFileSync('lua/utils/npm/tmp_cookies.json', JSON.stringify({
                    cookies: finalCookies,
                    ua: finalUA
                }));

            } catch (error) {
                console.log(JSON.stringify({ error: error.toString() }));
                process.exit(1); // Exit the process with error code
            } finally {
                if (browser) {
                    await browser.close();
                }
            }
        })();
    