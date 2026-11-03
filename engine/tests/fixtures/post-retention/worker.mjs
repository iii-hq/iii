import { registerWorker } from 'iii-sdk';
import { createHash } from 'node:crypto';
import { finished } from 'node:stream/promises';

const url = 'ws://127.0.0.1:49734';
const iii = registerWorker(url, { workerName: 'post-retention-probe' });
const hash = b => createHash('sha256').update(b).digest('hex');
iii.registerFunction('probe::buffered', async req => ({ status_code: 200, body: { ok: true, body: req.body } }));
iii.registerFunction('probe::read', async req => {
  const reader = req.request_body;
  const chunks = [];
  for await (const chunk of reader.stream) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  return { status_code: 200, body: { bytes: body.length, sha256: hash(body) } };
});
iii.registerFunction('probe::sse', async req => {
  const writer = req.response;
  writer.sendMessage(JSON.stringify({ type: 'set_headers', headers: { 'content-type': 'text/event-stream' } }));
  for (let i = 0; i < 5; i++) {
    writer.stream.write(Buffer.from(`data: ${i}\n\n`));
    await new Promise(r => setTimeout(r, 20));
  }
  writer.stream.end();
  await finished(writer.stream);
  return null;
});
for (const [method, path, function_id] of [
  ['GET', '/probe', 'probe::buffered'], ['POST', '/probe', 'probe::buffered'],
  ['POST', '/read', 'probe::read'], ['POST', '/sse', 'probe::sse'],
]) iii.observe({ function_id: 'http::on-request', args: { method, path } }, function_id);
console.log('Reproduction worker started');
