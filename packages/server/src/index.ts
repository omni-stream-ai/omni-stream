import { createServer } from 'node:http';
import { createStore } from '@omnistream/storage-sqlite';
const store = createStore();
const port = Number(process.env.PORT ?? 4317);
const server = createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  if (req.url === '/health') { res.end(JSON.stringify({ ok: true })); return; }
  if (req.method === 'GET' && req.url === '/projects') { res.end(JSON.stringify([...store.projects.values()])); return; }
  res.statusCode = 404; res.end(JSON.stringify({ error: 'not found' }));
});
server.listen(port, () => console.log(`omnistream server listening on http://127.0.0.1:${port}`));
