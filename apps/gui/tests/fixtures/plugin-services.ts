import { createServer, type Server } from 'node:http';
import { createHash } from 'node:crypto';
import type { Socket } from 'node:net';

export type ServiceRequest = { path: string; method: string; query: URLSearchParams; headers: Record<string, string | string[] | undefined>; body: Record<string, any> };
export async function pluginServices() {
  const requests: ServiceRequest[] = [];
  const sockets = new Set<Socket>();
  const acknowledgements: string[] = [];
  let socketConnections = 0;
  let eventSequence = 0;
  let messageSequence = 0;
  const frame = (payload: Buffer, opcode = 1) => {
    const header = Buffer.alloc(payload.length < 126 ? 2 : payload.length < 65536 ? 4 : 10);
    header[0] = 0x80 | opcode;
    if (header.length === 2) header[1] = payload.length;
    else if (header.length === 4) { header[1] = 126; header.writeUInt16BE(payload.length, 2); }
    else { header[1] = 127; header.writeBigUInt64BE(BigInt(payload.length), 2); }
    return Buffer.concat([header, payload]);
  };
  const slackEvent = (event: Record<string, unknown>, options: { team?: string; owner?: string; eventId?: string } = {}) => {
    const sequence = ++eventSequence;
    const envelopeId = 'envelope-' + sequence;
    const payload = { envelope_id: envelopeId, type: 'events_api', payload: { type: 'event_callback', team_id: options.team ?? 'T123', event_id: options.eventId ?? 'event-' + sequence, authorizations: [{ user_id: options.owner ?? 'U123', team_id: options.team ?? 'T123', is_bot: false }], event } };
    for (const socket of sockets) socket.write(frame(Buffer.from(JSON.stringify(payload))));
    return envelopeId;
  };
  const gates = new Map<string, { entered: () => void; released: Promise<void> }>();
  const releases = new Set<() => void>();
  const hold = (method: string, path: string) => {
    let entered!: () => void; let resume!: () => void;
    const started = new Promise<void>(resolve => { entered = resolve; });
    const gate = { entered, released: new Promise<void>(resolve => { resume = resolve; }) };
    const key = method + ' ' + path;
    const release = () => { if (gates.get(key) === gate) gates.delete(key); releases.delete(release); resume(); };
    releases.add(release); gates.set(key, gate);
    return { started, release };
  };
  const server: Server = createServer((request, response) => {
    let raw = '';
    request.on('data', chunk => { raw += chunk; });
    request.on('end', async () => {
      const url = new URL(request.url ?? '/', 'http://localhost');
      const path = url.pathname;
      const body = raw ? JSON.parse(raw) : {};
      requests.push({ path, body, method: request.method ?? '', query: url.searchParams, headers: request.headers });
      const key = request.method + ' ' + path;
      const gate = gates.get(key);
      if (gate) { gates.delete(key); gate.entered(); await gate.released; }
      if (response.destroyed) return;
      response.setHeader('Content-Type', 'application/json');
      const send = (value: unknown) => response.end(JSON.stringify(value));
      if (path === '/model/models') return send({ data: [{ id: 'fixture' }] });
      if (path === '/model/responses') {
        const input = String(body.input ?? '');
        const prompt = input.slice(input.lastIndexOf('用户输入：'));
        const slack = prompt.includes('Slack');
        const write = prompt.includes('创建 GitHub Issue') || prompt.includes('发送 Slack');
        const done = input.includes('工具调用 plugin_invoke 参数');
        const decision = done ? { kind: 'reply', message: input.includes('工具结果 {"error":') ? '插件操作失败。' : write ? '插件写入已完成。' : '插件读取已完成。' } : {
          kind: 'tool', name: 'plugin_invoke', args: slack
            ? { plugin: 'slack', action: write ? 'send_message' : 'channel_history', input: write ? { channel: 'D123', text: 'Assistant personal message' } : { channel: 'D123' } }
            : { plugin: 'github', action: write ? 'create_issue' : 'list_issues', input: write ? { owner: 'org', repo: 'project', title: 'Assistant issue', body: 'Created through chat' } : { owner: 'org', repo: 'project' } },
        };
        return send({ output_text: JSON.stringify(decision) });
      }
      const token = String(request.headers.authorization ?? request.headers['x-figma-token'] ?? '');
      if (token.includes('bad-token')) { response.statusCode = 401; return send({ message: 'Invalid ' + token }); }
      const slackUser = token.startsWith('Bearer xoxp-') && !token.includes('bot-identity');
      if (path === '/slack/auth.test') return send(slackUser ? { ok: true, user: token.includes('other-user') ? 'other-user' : 'fixture-user', user_id: token.includes('other-user') ? 'UOTHER' : token.includes('stream-user') ? 'USTREAM' : 'U123', team_id: 'T123' } : { ok: true, user: 'team-bot', user_id: 'UBOT', bot_id: 'B123' });
      if (path === '/slack/apps.connections.open') {
        if (!token.startsWith('Bearer xapp-') || token.includes('invalid-app')) { response.statusCode = 401; return send({ ok: false, error: 'invalid_auth' }); }
        return send({ ok: true, url: origin.replace('http:', 'ws:') + '/slack/socket' });
      }
      if (path === '/github/user') return send({ login: 'octocat' });
      if (path === '/notion/users/me') return send({ name: 'Notes bot', id: 'bot-1' });
      if (path === '/figma/me') return send({ handle: 'designer' });
      if (path === '/slack/conversations.list') {
        const type = url.searchParams.get('types');
        const channels = type === 'im' ? [{ id: 'D123', is_im: true, user: 'U456' }] : type === 'mpim' ? [{ id: 'G123', name: 'group-dm', is_mpim: true }] : type === 'private_channel' ? [{ id: 'GPRIVATE', name: 'private-team', is_private: true }] : [{ id: 'C123', name: 'general' }];
        return send({ ok: true, channels, response_metadata: { next_cursor: url.searchParams.has('cursor') ? '' : 'cursor-next' } });
      }
      if (path === '/slack/conversations.history') {
        if (url.searchParams.get('channel') === 'CUNJOINED' && !slackUser) return send({ ok: false, error: 'not_in_channel' });
        return send({ ok: true, messages: [{ text: url.searchParams.get('channel') === 'D123' ? 'Personal DM update' : 'Team update' }] });
      }
      if (path === '/slack/conversations.replies') return send({ ok: true, messages: [{ text: 'Thread reply', ts: '123.457' }], response_metadata: { next_cursor: url.searchParams.has('cursor') ? '' : 'thread-next' } });
      if (path === '/slack/conversations.open') return send({ ok: true, channel: { id: body.users.includes(',') ? 'G123' : 'D123' } });
      if (path === '/slack/chat.postMessage') return send({ ok: true, channel: body.channel, ts: (1700000100 + ++messageSequence) + '.000001', message: { text: body.text } });
      if (path === '/github/user/repos') return send([{ name: 'Fixture repository', full_name: 'org/project', html_url: 'https://github.com/org/project' }]);
      if (path === '/github/repos/org/project/issues') return send(request.method === 'POST' ? { number: 2, title: body.title, body: body.body, html_url: 'https://github.com/org/project/issues/2' } : [{ number: 1, title: 'fixture_issue', state: 'open', html_url: 'https://github.com/org/project/issues/1' }]);
      if (path === '/github/repos/org/project/pulls') return send([{ number: 3, title: 'Fixture PR' }]);
      if (path === '/github/repos/org/project') return send({ name: 'Fixture repository' });
      if (path === '/notion/search') return send({ results: [{ id: 'page-1', url: 'https://notion.so/page1', properties: { title: { type: 'title', title: [{ plain_text: 'Team notes' }] } } }], next_cursor: null });
      if (path === '/notion/pages' && request.method === 'POST') return send({ id: 'page-2', properties: body.properties, url: 'https://notion.so/page2' });
      if (path === '/notion/pages/page-1') return send({ id: 'page-1', properties: { title: { type: 'title', title: [{ plain_text: 'Team notes' }] } } });
      if (path === '/notion/blocks/page-1/children') return send({ results: [{ id: 'block-1', type: 'paragraph', paragraph: { rich_text: [{ plain_text: 'Notes content' }] } }] });
      if (path === '/figma/files/File123') return send({ name: 'Fixture design', document: { children: [] } });
      if (path === '/figma/files/File123/comments') return send(request.method === 'POST' ? { id: 'comment-2', message: body.message } : { comments: [{ id: 'comment-1', message: 'Review note' }] });
      response.statusCode = 404; send({ message: 'Unknown fixture endpoint ' + path });
    });
  });
  server.on('upgrade', (request, stream) => {
    const socket = stream as Socket;
    if (request.url !== '/slack/socket' || !request.headers['sec-websocket-key']) { socket.destroy(); return; }
    const accept = createHash('sha1').update(String(request.headers['sec-websocket-key']) + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ' + accept + '\r\n\r\n');
    sockets.add(socket); socketConnections++;
    socket.write(frame(Buffer.from(JSON.stringify({ type: 'hello' }))));
    let pending = Buffer.alloc(0);
    socket.on('data', (data: Buffer) => {
      pending = Buffer.concat([pending, data]);
      while (pending.length >= 2) {
        const opcode = pending[0] & 15;
        const masked = (pending[1] & 128) !== 0;
        let length = pending[1] & 127; let offset = 2;
        if (length === 126) { if (pending.length < 4) return; length = pending.readUInt16BE(2); offset = 4; }
        else if (length === 127) { if (pending.length < 10) return; length = Number(pending.readBigUInt64BE(2)); offset = 10; }
        if (pending.length < offset + (masked ? 4 : 0) + length) return;
        const mask = masked ? pending.subarray(offset, offset + 4) : null;
        offset += masked ? 4 : 0;
        const payload = Buffer.from(pending.subarray(offset, offset + length));
        if (mask) for (let index = 0; index < payload.length; index++) payload[index] ^= mask[index % 4];
        pending = pending.subarray(offset + length);
        if (opcode === 9) socket.write(frame(payload, 10));
        else if (opcode === 8) { socket.end(frame(Buffer.alloc(0), 8)); return; }
        else if (opcode === 1) { const value = JSON.parse(payload.toString()); if (value.envelope_id) acknowledgements.push(value.envelope_id); }
      }
    });
    socket.on('close', () => sockets.delete(socket));
    socket.on('end', () => { sockets.delete(socket); socket.end(); });
    socket.on('error', () => { sockets.delete(socket); socket.destroy(); });
  });
  await new Promise<void>(done => server.listen(0, '127.0.0.1', done));
  const origin = 'http://127.0.0.1:' + (server.address() as { port: number }).port;
  return { origin, requests, hold, slackEvent, acknowledgements, get socketConnections() { return socketConnections; }, get activeSockets() { return sockets.size; }, disconnectSockets: () => { for (const socket of sockets) socket.destroy(); }, close: () => { for (const socket of sockets) socket.destroy(); for (const release of [...releases]) release(); return new Promise<void>((done, reject) => server.close(error => error ? reject(error) : done())); } };
}
