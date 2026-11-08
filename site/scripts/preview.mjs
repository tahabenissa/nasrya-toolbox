// Serve .output/public under the GitHub Pages base path, like Pages does:
//   NUXT_APP_BASE_URL=/winsploit/ npm run generate && npm run preview
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { join, resolve, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../.output/public');
const base = (process.env.NUXT_APP_BASE_URL || '/winsploit/').replace(/\/?$/, '/');
const port = Number(process.env.PORT || 4173);
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.txt': 'text/plain' };

createServer(async (req, res) => {
  const url = new URL(req.url, 'http://x');
  if (!url.pathname.startsWith(base)) {
    res.writeHead(302, { Location: base }).end();
    return;
  }
  const rel = decodeURIComponent(url.pathname.slice(base.length));
  if (rel.includes('..')) return res.writeHead(400).end();
  for (const c of [rel, join(rel, 'index.html'), `${rel}.html`]) {
    const f = join(root, c);
    try {
      if ((await stat(f)).isFile()) {
        res.writeHead(200, { 'Content-Type': TYPES[extname(f)] || 'application/octet-stream' });
        res.end(await readFile(f));
        return;
      }
    } catch { /* try the next candidate */ }
  }
  res.writeHead(404, { 'Content-Type': 'text/html' }).end(await readFile(join(root, '404.html')).catch(() => 'not found'));
}).listen(port, () => console.log(`preview: http://localhost:${port}${base}`));
