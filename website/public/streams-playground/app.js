/* Streams playground (English): plain JS, no build step besides the highlighted snippets in snippets.js. */
(() => {
  'use strict'

  const S = window.SNIPPETS
  const $ = (sel, root = document) => root.querySelector(sel)
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)]
  const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches
  const wait = (ms) => new Promise((r) => setTimeout(r, reduced() ? 0 : ms))
  const h = (html) => {
    const t = document.createElement('template')
    t.innerHTML = html.trim()
    return t.content.firstElementChild
  }
  const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

  /* ---------- theme ---------- */
  $('#theme').addEventListener('click', () => {
    const dark = document.documentElement.classList.toggle('dark')
    try { localStorage.setItem('theme', dark ? 'dark' : 'light') } catch {}
  })

  /* ---------- pixel headings print in when they enter the view ---------- */
  const io = new IntersectionObserver(
    (entries) => entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('in-view'); io.unobserve(e.target) } }),
    { threshold: 0.6 },
  )
  $$('.pixel').forEach((el) => io.observe(el))

  /* ---------- code ---------- */
  function codeHTML(snippet, { numbered = false } = {}) {
    return snippet.lines
      .map((line, i) => `<span class="ln"${numbered ? ` data-n="${i + 1}"` : ''} data-i="${i}">${line || ' '}</span>`)
      .join('')
  }
  function fillSnippets(root = document) {
    $$('[data-snippet]', root).forEach((el) => {
      const s = S.ts[el.dataset.snippet]
      if (s) el.innerHTML = codeHTML(s)
    })
  }
  fillSnippets()

  /* =====================================================================
   * PLAYGROUND
   * ===================================================================== */

  const LANGS = [
    { id: 'ts', label: 'TypeScript', ext: 'ts' },
    { id: 'py', label: 'Python', ext: 'py' },
    { id: 'rs', label: 'Rust', ext: 'rs' },
    { id: 'go', label: 'Go', ext: 'go' },
  ]

  const LOG = [
    { t: '09:00 INFO  server started' },
    { t: '09:01 INFO  GET /health 200' },
    { t: '09:02 ERROR disk is full', err: true },
    { t: '09:03 INFO  GET /orders 200' },
    { t: '09:04 ERROR db timeout', err: true },
    { t: '09:05 INFO  shutting down' },
  ]
  const TABLE = Array.from({ length: 10 }, (_, i) => ({ t: `7 x ${i + 1} = ${7 * (i + 1)}` }))
  const JOBS = Array.from({ length: 5 }, (_, i) => ({ t: `{ done: ${i + 1} }`, kind: 'msg' }))

  const ref = (field, direction, extra = '') =>
    `${extra}<span class="k">${field}:</span> {\n  <span class="k">channel_id:</span> <span class="s">"7f3a…"</span>,\n  <span class="k">access_key:</span> <span class="s">"••••"</span>,\n  <span class="k">direction:</span> <span class="s">"${direction}"</span> }`

  const tag = (text, tone = '') => `<span class="tag"${tone ? ` data-tone="${tone}"` : ''}>${text}</span>`
  const fileView = (name, items, delivered, { mode = 'source', limit = 99 } = {}) => {
    const rows = items
      .slice(0, limit)
      .map((it, i) => {
        let state = ''
        if (mode === 'source') state = i < delivered ? 'sent' : ''
        if (mode === 'sink') {
          if (i > delivered) return ''
          if (i === delivered) return `<span class="file-line ghost">${esc(it.t)}</span>`
          state = i === delivered - 1 ? 'new' : ''
        }
        if (mode === 'console') {
          if (i >= delivered) return ''
          state = it.err ? 'err' : i === delivered - 1 ? 'new' : ''
        }
        return `<span class="file-line" data-state="${state}">${esc(it.t)}</span>`
      })
      .join('')
    return `<div class="file"><div class="file-head"><span>${name}</span><span>${mode === 'source' ? `${items.length} lines` : ''}</span></div><div class="file-lines" data-mode="${mode}">${rows}</div></div>`
  }

  /* Each step: tag (code lines), who, title, caption, detail, deep (only "under the hood"), state(deep).
     State: active, pipe, bubble {id, at, from?, text, vanish?}, engine, delivered, phase. */
  const SCENARIOS = [
    {
      id: 'send',
      label: 'Send',
      icon: '→',
      fn: 'logs::count-errors',
      callerFn: 'iii.call(…)',
      files: { caller: 'caller', handler: 'worker' },
      flow: { dir: 'ab', items: LOG },
      viewA(st) {
        const done = st.phase === 'result'
        return {
          body: fileView('app.log', LOG, st.delivered, { mode: 'source' }),
          foot: done ? tag('{ errors: 2 }', 'ok') : st.phase === 'payload' ? tag('payload: { log: app.log }') : tag('waiting for the result…'),
        }
      },
      viewB(st) {
        if (!st.received) return { body: `<div class="big-label">waiting for a call</div>`, foot: '' }
        const errors = LOG.slice(0, st.delivered).filter((l) => l.err).length
        return {
          body: `<div class="stat"><span class="big-num">${errors}</span><span class="big-label">errors · ${st.delivered}/${LOG.length} lines read</span></div>${fileView('log.lines()', LOG, st.delivered, { mode: 'console' })}`,
          foot: tag('log: InputStream', 'accent') + (st.phase === 'end' || st.phase === 'result' ? tag('end of stream', 'ok') : ''),
        }
      },
      steps: [
        {
          tag: 'payload', who: 'caller',
          title: 'Put the file in the payload',
          caption: 'A file is a value like any other. There is no channel to create first.',
          detail: 'The SDK recognizes sources: a Node <code>Readable</code>, a <code>ReadableStream</code>, a <code>Blob</code> or <code>File</code>, an async generator, or an <code>InputStream</code>. Strings, buffers, objects, and arrays stay JSON.',
          state: () => ({ active: 'a', pipe: 'none', phase: 'payload', bubble: { id: 'req', at: 'a', text: '<span class="k">payload:</span> { <span class="k">log:</span> <span class="s">▤ app.log</span> }' } }),
        },
        {
          deep: true, tag: 'payload', who: 'sdk',
          title: 'The SDK creates a pipe and swaps the file for a ref',
          caption: 'A small JSON goes in place of the file. That is what the call carries.',
          detail: 'The SDK calls <code>engine::channels::create</code>: one pipe per stream, in parallel when there are several. If a pipe can\'t be created, the call fails before it leaves. The ref carries the access key; an observer of the call gets only a <code>StreamInfo</code>, without the key.',
          state: () => ({ active: 'a', pipe: 'ready', phase: 'payload', engine: 'channels::create', bubble: { id: 'req', at: 'a', text: ref('log', 'read') } }),
        },
        {
          tag: 'receive', who: 'handler',
          title: 'The handler gets an InputStream',
          caption: 'The call arrived. The <code>log</code> field is ready to read, and no bytes have moved yet.',
          detail: 'The handler\'s SDK turns the ref back into an <code>InputStream</code> before the handler runs. Each end opens its socket on the first read or write.',
          state: (deep) => ({ active: 'b', pipe: 'ready', received: true, bubble: { id: 'req', at: 'b', vanish: true, text: deep ? ref('log', 'read') : '<span class="k">payload:</span> { <span class="k">log:</span> <span class="s">▤ app.log</span> }' } }),
        },
        {
          tag: 'read', who: 'both',
          title: 'The bytes flow while both functions run',
          caption: 'The caller\'s SDK pumps the file through the pipe. The handler reads it line by line.',
          detail: 'There is backpressure: each write only finishes when the chunk is accepted, so a slow reader slows the writer down instead of filling up memory. The engine holds up to 64 frames between the ends.',
          state: () => ({ active: 'both', pipe: 'flow', received: true, delivered: LOG.length }),
        },
        {
          tag: 'end', who: 'handler',
          title: 'The file ended: the stream closes and the loop finishes',
          caption: 'A clean end means a complete stream. The handler returns <code>{ errors: 2 }</code>.',
          detail: 'If the file failed halfway, the stream would abort and the loop would throw <code>STREAM_ABORTED</code>, after the data that had already arrived. A cut stream never looks complete.',
          state: () => ({ active: 'b', pipe: 'closed', received: true, delivered: LOG.length, phase: 'end' }),
        },
        {
          tag: 'result', who: 'caller',
          title: 'The result comes back as JSON',
          caption: 'A regular call: <code>errors</code> is 2. The caller didn\'t close anything.',
          detail: 'The caller\'s SDK read the file to the end and closed the stream. In Go, <code>InputStreamFromReader</code> also closes the file at the end, when it is an <code>io.Closer</code>.',
          state: () => ({ active: 'a', pipe: 'closed', received: true, delivered: LOG.length, phase: 'result', bubble: { id: 'res', at: 'a', from: 'b', text: '{ <span class="k">errors:</span> 2 }' } }),
        },
      ],
    },

    {
      id: 'return',
      label: 'Return',
      icon: '←',
      fn: 'logs::read',
      callerFn: 'iii.call(…)',
      files: { caller: 'caller', handler: 'worker' },
      flow: { dir: 'ba', items: LOG },
      viewA(st) {
        if (!st.received) return { body: `<div class="big-label">${st.phase === 'call' ? 'calling…' : 'waiting for the result…'}</div>`, foot: '' }
        return {
          body: fileView('body.lines()', LOG, st.delivered, { mode: 'console' }) + (st.delivered === 0 ? '<div class="big-label">the call already returned; the bytes come next</div>' : ''),
          foot: tag('name: "app.log"') + tag('body: InputStream', 'accent') + (st.phase === 'end' ? tag('end of stream', 'ok') : ''),
        }
      },
      viewB(st) {
        if (st.phase === 'call') return { body: `<div class="big-label">getting the call</div>`, foot: '' }
        return {
          body: fileView('app.log', LOG, st.delivered, { mode: 'source' }),
          foot: st.answered ? tag('answered · pumping', st.delivered < LOG.length ? 'accent' : '') : '',
        }
      },
      steps: [
        {
          tag: 'call', who: 'caller',
          title: 'A regular call',
          caption: 'The request carries no stream: <code>payload: {}</code>.',
          detail: 'Streams can go both ways: in the payload, from the caller to the handler, or in the result, from the handler to the caller.',
          state: () => ({ active: 'a', pipe: 'none', phase: 'call', bubble: { id: 'req', at: 'b', from: 'a', vanish: true, text: '<span class="k">payload:</span> {}' } }),
        },
        {
          tag: 'return', who: 'handler',
          title: 'The handler returns the file in the result',
          caption: '<code>name</code> is JSON; <code>body</code> is a stream. Both in the same object.',
          detail: 'The SDK recognizes the source in the result the same way it does in the payload: a file, a <code>Readable</code>, an async generator, a <code>ReadableStream</code>.',
          state: () => ({ active: 'b', pipe: 'none', phase: 'return', bubble: { id: 'res', at: 'b', text: '{ <span class="k">name:</span> <span class="s">"app.log"</span>,\n  <span class="k">body:</span> <span class="s">▤ app.log</span> }' } }),
        },
        {
          deep: true, tag: 'return', who: 'sdk',
          title: 'The handler\'s SDK creates the pipe and puts the ref in the result',
          caption: 'Whoever returns the stream owns the pipe.',
          detail: 'The owning worker has to stay connected until the caller connects to its end. An end nobody opens expires after about five minutes.',
          state: () => ({ active: 'b', pipe: 'ready', phase: 'return', engine: 'channels::create', bubble: { id: 'res', at: 'b', text: '{ <span class="k">name:</span> <span class="s">"app.log"</span>,\n  ' + ref('body', 'read').replace(/\n/g, '\n  ') + ' }' } }),
        },
        {
          tag: 'receive', who: 'caller',
          title: 'The call resolves before the data',
          caption: '<code>name</code> is already here. <code>body</code> arrives as an <code>InputStream</code>, and the bytes come next.',
          detail: 'New in 0.27: the SDK also turns refs into streams in a call\'s result, not only in the handler\'s input.',
          state: (deep) => ({ active: 'a', pipe: 'ready', received: true, answered: true, phase: 'receive', bubble: { id: 'res', at: 'a', vanish: true, text: deep ? '{ <span class="k">name:</span> <span class="s">"app.log"</span>,\n  ' + ref('body', 'read').replace(/\n/g, '\n  ') + ' }' : '{ <span class="k">name:</span> <span class="s">"app.log"</span>,\n  <span class="k">body:</span> <span class="s">▤ app.log</span> }' } }),
        },
        {
          tag: 'read', who: 'both',
          title: 'The caller reads while the handler pumps',
          caption: 'The handler\'s SDK reads the file and sends it through the pipe, after it has already answered.',
          detail: 'The handler already returned; its SDK is what keeps working, pumping the source to the end.',
          state: () => ({ active: 'both', pipe: 'flow', received: true, answered: true, delivered: LOG.length, phase: 'read' }),
        },
        {
          tag: 'end', who: 'caller',
          title: 'End of file, end of loop',
          caption: 'The stream closed cleanly: the caller read everything.',
          detail: 'If the caller stops halfway (a <code>break</code>, or <code>cancel()</code>), the stream is cancelled and the engine closes the writer.',
          state: () => ({ active: 'a', pipe: 'closed', received: true, answered: true, delivered: LOG.length, phase: 'end' }),
        },
      ],
    },

    {
      id: 'sink',
      label: 'Write to a destination',
      icon: '⤓',
      fn: 'math::table',
      callerFn: 'iii.call(…)',
      files: { caller: 'caller', handler: 'worker' },
      flow: { dir: 'ba', items: TABLE },
      viewA(st) {
        return {
          body: fileView('table.txt', TABLE, st.delivered, { mode: 'sink' }),
          foot: st.phase === 'result' ? tag('resolved · file complete', 'ok') : st.phase === 'payload' ? tag('payload: { n: 7, out: table.txt }') : tag('the call waits…'),
        }
      },
      viewB(st) {
        if (!st.received) return { body: `<div class="big-label">waiting for a call</div>`, foot: '' }
        return {
          body: `<div class="stat"><span class="big-num">${st.delivered}</span><span class="big-label">× out.write(…) · n = 7</span></div>`,
          foot: tag('out: OutputStream', 'warn') + (st.phase === 'close' || st.phase === 'result' ? tag('returned → closed', 'ok') : ''),
        }
      },
      steps: [
        {
          tag: 'payload', who: 'caller',
          title: 'Pass where the bytes should land',
          caption: 'A file open for writing is a destination (a <em>sink</em>).',
          detail: 'In Python it is <code>OutputStream.to_file()</code>; in Rust, <code>OutputStream::to_writer()</code>; in Go, <code>iii.OutputStreamToWriter()</code>. In Node, a <code>Writable</code> or a <code>WritableStream</code> (a <code>Duplex</code> doesn\'t count).',
          state: () => ({ active: 'a', pipe: 'none', phase: 'payload', bubble: { id: 'req', at: 'a', text: '<span class="k">payload:</span> { <span class="k">n:</span> 7, <span class="k">out:</span> <span class="s">▤ table.txt</span> }' } }),
        },
        {
          deep: true, tag: 'payload', who: 'sdk',
          title: 'The SDK sends a write ref and copies whatever arrives into the file',
          caption: 'The ref goes with <code>direction: "write"</code>: on the other side it becomes an <code>OutputStream</code>.',
          detail: 'The caller\'s SDK keeps reading the other end of the pipe and writing to <code>table.txt</code>.',
          state: () => ({ active: 'a', pipe: 'ready', phase: 'payload', engine: 'channels::create', bubble: { id: 'req', at: 'a', text: '{ <span class="k">n:</span> 7,\n  ' + ref('out', 'write').replace(/\n/g, '\n  ') + ' }' } }),
        },
        {
          tag: 'receive', who: 'handler',
          title: 'The handler gets an OutputStream',
          caption: 'It only writes. The caller doesn\'t have to do anything but wait.',
          detail: 'The pipe\'s socket opens on the first write. A handler that writes nothing and returns closes the stream empty, so the caller\'s destination finishes right away.',
          state: (deep) => ({ active: 'b', pipe: 'ready', received: true, bubble: { id: 'req', at: 'b', vanish: true, text: deep ? '{ <span class="k">n:</span> 7,\n  ' + ref('out', 'write').replace(/\n/g, '\n  ') + ' }' : '<span class="k">payload:</span> { <span class="k">n:</span> 7, <span class="k">out:</span> <span class="s">▤ table.txt</span> }' } }),
        },
        {
          tag: 'write', who: 'both',
          title: 'Each write lands in the caller\'s file',
          caption: 'The caller\'s SDK copies each chunk into <code>table.txt</code>.',
          detail: 'A destination only gets bytes: messages sent with <code>send</code> are dropped. To get messages, pass the end of a <code>pipe()</code>.',
          state: () => ({ active: 'both', pipe: 'flow', received: true, delivered: TABLE.length }),
        },
        {
          tag: 'close', who: 'handler',
          title: 'Returning closes the stream',
          caption: 'If the handler threw an error, the stream would abort and the call would fail with <code>STREAM_ABORTED</code>.',
          detail: 'The call also fails with <code>STREAM_ABORTED</code> when the destination itself fails (a full disk, for example), even if the function returned. In both cases the file may hold only part of the data: delete it.',
          state: () => ({ active: 'b', pipe: 'closed', received: true, delivered: TABLE.length, phase: 'close' }),
        },
        {
          tag: 'result', who: 'caller',
          title: 'The call only resolves when the file has everything',
          caption: 'When the <code>await</code> returns, <code>table.txt</code> is complete.',
          detail: 'The call timeout also covers the copy into the destination. If it expires first, the SDK aborts the destination and the call fails with <code>TIMEOUT</code>.',
          state: () => ({ active: 'a', pipe: 'closed', received: true, delivered: TABLE.length, phase: 'result', bubble: { id: 'res', at: 'a', from: 'b', text: '<span class="s">✓</span> resolved' } }),
        },
      ],
    },

    {
      id: 'pipe',
      label: 'pipe()',
      icon: '⇄',
      code: true,
      fn: 'jobs::run',
      callerFn: 'iii.pipe() + iii.call(…)',
      files: { caller: 'caller', handler: 'worker' },
      flow: { dir: 'ba', items: JOBS, kind: 'msg' },
      viewA(st) {
        if (!st.hasPipe) return { body: `<div class="big-label">no pipe yet</div>`, foot: '' }
        const pct = Math.round((st.delivered / JOBS.length) * 100)
        return {
          body: `<div class="stat"><span class="big-num">${st.delivered}/5</span><span class="big-label">input.messages()</span></div><div class="progress"><span style="width:${pct}%"></span></div>`,
          foot: tag('input', 'accent') + tag(st.handedOut ? 'output → payload' : 'output', 'warn') + (st.phase === 'result' ? tag('{ ok: true }', 'ok') : st.phase === 'close' ? tag('loop finished', 'ok') : ''),
        }
      },
      viewB(st) {
        if (!st.received) return { body: `<div class="big-label">waiting for a call</div>`, foot: '' }
        return {
          body: `<div class="stat"><span class="big-num">${st.delivered}</span><span class="big-label">× progress.send(…)</span></div>`,
          foot: tag('progress: OutputStream', 'warn') + (st.phase === 'close' || st.phase === 'result' ? tag('returned → closed', 'ok') : ''),
        }
      },
      steps: [
        {
          tag: 'pipe', who: 'caller',
          title: 'Create a pipe: you hold both ends',
          caption: '<code>input</code> to read. <code>output</code> to hand to whoever will write.',
          detail: 'Whoever calls <code>pipe()</code> owns the pipe. The engine holds up to 64 frames between the ends; in TypeScript, <code>pipe({ bufferSize })</code> changes that.',
          state: (deep) => ({ active: 'a', pipe: 'ready', hasPipe: true, phase: 'pipe', engine: deep ? 'channels::create' : null }),
        },
        {
          tag: 'payload', who: 'caller',
          title: 'Hand the output over in the payload, without awaiting the call',
          caption: 'No <code>await</code> here: you want to read while the call runs.',
          detail: 'Mark the promise as handled (<code>done.catch(() =&gt; {})</code>): if the call fails while the loop runs, the <code>await done</code> at the end still throws the error.',
          state: (deep) => ({ active: 'a', pipe: 'ready', hasPipe: true, handedOut: true, phase: 'payload', bubble: { id: 'req', at: 'b', from: 'a', vanish: true, text: deep ? '{ <span class="k">steps:</span> 5,\n  ' + ref('progress', 'write').replace(/\n/g, '\n  ') + ' }' : '<span class="k">payload:</span> { <span class="k">steps:</span> 5, <span class="k">progress:</span> <span class="s">output</span> }' } }),
        },
        {
          tag: 'receive', who: 'handler',
          title: 'The handler gets an OutputStream',
          caption: 'To the handler, it looks the same as a regular destination.',
          detail: 'The handler doesn\'t know whether the other side is a file or a pipe\'s <code>input</code>. It writes the same way.',
          state: () => ({ active: 'b', pipe: 'ready', hasPipe: true, handedOut: true, received: true, phase: 'receive' }),
        },
        {
          tag: 'send', who: 'both',
          title: 'JSON messages arrive one by one',
          caption: '<code>send</code> sends a JSON value; <code>messages()</code> yields each one already parsed.',
          detail: 'A stream carries bytes and messages in the same order. With a destination (a file), the messages would be dropped; on a pipe\'s <code>input</code>, they arrive.',
          state: () => ({ active: 'both', pipe: 'flow', hasPipe: true, handedOut: true, received: true, delivered: JOBS.length, phase: 'send' }),
        },
        {
          tag: 'close', who: 'handler',
          title: 'The handler returns: the stream closes and the loop finishes',
          caption: 'Returning closes <code>progress</code>. On the other side, the <code>messages()</code> loop ends.',
          detail: 'If the handler threw an error after writing, the stream would abort and the loop would throw <code>STREAM_ABORTED</code>. If it threw before writing, the loop would wait: that is why you should observe the call too.',
          state: () => ({ active: 'b', pipe: 'closed', hasPipe: true, handedOut: true, received: true, delivered: JOBS.length, phase: 'close' }),
        },
        {
          tag: 'result', who: 'caller',
          title: 'The call resolves with the result',
          caption: '<code>await done</code> returns <code>{ ok: true }</code>.',
          detail: 'With a destination, the call would wait for the stream to finish. With <code>pipe()</code>, it resolves when the handler returns. Here both happen almost together, because the handler returns after the last message.',
          state: () => ({ active: 'a', pipe: 'closed', hasPipe: true, handedOut: true, received: true, delivered: JOBS.length, phase: 'result', bubble: { id: 'res', at: 'a', from: 'b', text: '{ <span class="k">ok:</span> true }' } }),
        },
      ],
    },
  ]

  const WHO = { caller: 'caller', handler: 'handler', both: 'both sides', sdk: 'under the hood · SDK' }
  const BUBBLE_POS = { a: { left: '35%', x: '0%' }, mid: { left: '50%', x: '-50%' }, b: { left: '65%', x: '-100%' } }

  const pg = {
    scenario: SCENARIOS[0],
    lang: 'ts',
    deep: false,
    index: 0,
    playing: false,
    run: 0,
    token: 0,
  }

  const stage = $('#pg-stage')

  function steps() {
    return pg.scenario.steps.filter((s) => pg.deep || !s.deep)
  }
  function stateAt(i) {
    const st = steps()[i].state(pg.deep)
    return { delivered: 0, pipe: 'none', ...st }
  }

  function buildStage() {
    const sc = pg.scenario
    stage.innerHTML = `
      <div class="node" data-side="a">
        <div class="node-top"><span class="node-role">caller</span><span class="node-name">Worker A</span></div>
        <div class="node-fn">${esc(sc.callerFn)}</div>
        <div class="node-body"></div><div class="node-foot"></div>
      </div>
      <div class="track track-call"><div class="track-line"></div><span class="track-label">call · JSON</span></div>
      <div class="track track-pipe"><div class="track-line"></div><span class="track-label">pipe · ${sc.flow.kind === 'msg' ? 'messages' : 'bytes'}</span></div>
      <div class="engine">engine</div>
      <div class="node" data-side="b">
        <div class="node-top"><span class="node-role">handler</span><span class="node-name">Worker B</span></div>
        <div class="node-fn">${esc(sc.fn)}</div>
        <div class="node-body"></div><div class="node-foot"></div>
      </div>`
  }

  function paintNodes(st) {
    const sc = pg.scenario
    for (const side of ['a', 'b']) {
      const node = $(`.node[data-side="${side}"]`, stage)
      const v = side === 'a' ? sc.viewA(st) : sc.viewB(st)
      $('.node-body', node).innerHTML = v.body
      $('.node-foot', node).innerHTML = v.foot
      $$('.file-lines[data-mode="console"], .file-lines[data-mode="sink"]', node).forEach((l) => { l.scrollTop = l.scrollHeight })
      node.dataset.active = String(st.active === side || st.active === 'both')
    }
  }

  function paintStatic(st) {
    $('.track-pipe', stage).dataset.state = st.pipe === 'none' ? '' : st.pipe
    $$('.engine-note, .bubble, .chunk', stage).forEach((n) => n.remove())
    if (st.engine) stage.append(h(`<div class="engine-note">${st.engine}</div>`))
    if (st.bubble && !st.bubble.vanish) {
      const b = h(`<div class="bubble">${st.bubble.text}</div>`)
      const p = BUBBLE_POS[st.bubble.at]
      b.style.left = p.left
      b.style.translate = `${p.x} -50%`
      stage.append(b)
    }
    paintNodes(st)
  }

  function placeBubble(bubble) {
    const b = h(`<div class="bubble">${bubble.text}</div>`)
    const p = BUBBLE_POS[bubble.at]
    b.style.left = p.left
    b.style.translate = `${p.x} -50%`
    stage.append(b)
    return b
  }

  async function moveBubble(bubble, from, run) {
    const b = placeBubble({ ...bubble, at: from })
    const a = BUBBLE_POS[from]
    const z = BUBBLE_POS[bubble.at]
    const anim = b.animate(
      [
        { left: a.left, translate: `${a.x} -50%` },
        { left: z.left, translate: `${z.x} -50%` },
      ],
      { duration: 1000, easing: 'cubic-bezier(0.65, 0, 0.35, 1)', fill: 'forwards' },
    )
    await anim.finished.catch(() => {})
    if (run !== pg.run) return
    if (bubble.vanish) {
      await b.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 300, fill: 'forwards' }).finished.catch(() => {})
      b.remove()
    }
  }

  async function animateFlow(prev, st, run) {
    const sc = pg.scenario
    const from = prev ? prev.delivered || 0 : 0
    const to = st.delivered || 0
    if (to <= from) return
    const items = sc.flow.items
    const [x0, x1] = sc.flow.dir === 'ab' ? ['34%', '66%'] : ['66%', '34%']
    const travel = 900
    const gap = items.length > 6 ? 300 : 420
    const arrivals = []
    for (let i = from; i < to; i++) {
      if (run !== pg.run) return
      const it = items[i]
      const kind = it.kind || (it.err ? 'err' : '')
      const c = h(`<div class="chunk"${kind ? ` data-kind="${kind}"` : ''}>${kind === 'msg' ? esc(it.t) : ''}</div>`)
      c.style.left = x0
      stage.append(c)
      const anim = c.animate([{ left: x0, opacity: 0.2 }, { left: x0, opacity: 1, offset: 0.08 }, { left: x1, opacity: 1 }], {
        duration: travel,
        easing: 'cubic-bezier(0.65, 0, 0.35, 1)',
        fill: 'forwards',
      })
      arrivals.push(
        anim.finished
          .then(() => {
            c.remove()
            if (run !== pg.run) return
            paintNodes({ ...st, delivered: i + 1 })
          })
          .catch(() => c.remove()),
      )
      await wait(gap)
    }
    await Promise.all(arrivals)
  }

  async function goTo(i, { animate = true } = {}) {
    const list = steps()
    pg.index = Math.max(0, Math.min(i, list.length - 1))
    const run = ++pg.run
    const step = list[pg.index]
    const st = stateAt(pg.index)
    const prev = pg.index > 0 ? stateAt(pg.index - 1) : null
    paintText(step)
    paintCode(step)
    paintDots()
    if (!animate || reduced()) {
      paintStatic(st)
      return run
    }
    // Start from the previous step's nodes, with this step's pipe and engine, then animate into this step.
    const before = prev ? { ...prev, active: st.active } : { ...st, delivered: 0, received: false }
    paintStatic({ ...st, bubble: null })
    paintNodes(before)
    const nb = st.bubble
    if (nb) {
      const pb = prev && prev.bubble
      const from = nb.from || (pb && pb.id === nb.id && !pb.vanish && pb.at !== nb.at ? pb.at : null)
      if (from) await moveBubble(nb, from, run)
      else if (!nb.vanish) placeBubble(nb)
    }
    if (run !== pg.run) return run
    paintNodes({ ...st, delivered: prev ? prev.delivered || 0 : 0 })
    await animateFlow(prev, st, run)
    if (run !== pg.run) return run
    paintNodes(st)
    return run
  }

  function paintText(step) {
    const list = steps()
    $('#pg-count').textContent = `Step ${pg.index + 1} of ${list.length}`
    const title = $('#pg-title')
    title.innerHTML = `<span class="tag pg-step-who"${step.who === 'sdk' ? ' data-tone="warn"' : step.who === 'both' ? ' data-tone="accent"' : ''}>${WHO[step.who]}</span>${esc(step.title)}`
    title.style.animation = 'none'; void title.offsetWidth; title.style.animation = ''
    const cap = $('#pg-caption')
    cap.innerHTML = step.caption
    cap.style.animation = 'none'; void cap.offsetWidth; cap.style.animation = ''
    $('#pg-detail').innerHTML = `<p>${step.detail}</p>`
    $('#pg-prev').disabled = pg.index === 0
    $('#pg-next').disabled = pg.index === list.length - 1
  }

  function paintDots() {
    const list = steps()
    const ol = $('#pg-dots')
    ol.innerHTML = list
      .map((s, i) => `<li data-deep="${!!s.deep}" data-done="${i < pg.index}" data-current="${i === pg.index}"><button type="button" aria-label="Step ${i + 1}: ${esc(s.title)}"></button></li>`)
      .join('')
    $$('button', ol).forEach((b, i) => b.addEventListener('click', () => { stop(); goTo(i) }))
  }

  function paintCode(step) {
    const code = S.playground[pg.scenario.id][pg.lang]
    const ext = LANGS.find((l) => l.id === pg.lang).ext
    for (const side of ['caller', 'handler']) {
      const el = $(`#pg-${side}-code`)
      const snip = code[side]
      if (el.dataset.key !== `${pg.scenario.id}:${pg.lang}`) {
        el.innerHTML = codeHTML(snip, { numbered: true })
        el.dataset.key = `${pg.scenario.id}:${pg.lang}`
      }
      const hits = snip.tags.map((t) => t.includes(step.tag))
      const any = hits.some(Boolean)
      el.dataset.focus = any ? 'true' : 'idle'
      $$('.ln', el).forEach((ln, i) => { ln.dataset.hl = String(hits[i]) })
      $(`#pg-${side}`).dataset.active = String(any)
      $(`#pg-${side}-file`).textContent = `${pg.scenario.files[side]}.${ext}`
    }
  }

  function setPlaying(on) {
    pg.playing = on
    if (!on) pg.token = (pg.token || 0) + 1
    const b = $('#pg-play')
    b.dataset.playing = String(on)
    $('span', b).textContent = on ? 'Pause' : pg.index === steps().length - 1 ? 'Play again' : 'Play'
  }
  function stop() { if (pg.playing) setPlaying(false) }

  async function play() {
    if (pg.index >= steps().length - 1) await goTo(0, { animate: false })
    setPlaying(true)
    const token = (pg.token = (pg.token || 0) + 1)
    const alive = () => pg.playing && token === pg.token
    // the first press plays the current step, then each next one
    let first = true
    while (alive()) {
      if (!first) {
        if (pg.index >= steps().length - 1) break
        await goTo(pg.index + 1)
      } else {
        await goTo(pg.index)
        first = false
      }
      if (!alive()) return
      await wait(1900)
    }
    if (token === pg.token) setPlaying(false)
  }

  function buildTabs() {
    const scs = $('#pg-scenarios')
    scs.innerHTML = SCENARIOS.map((s) => `<button role="tab" type="button" data-id="${s.id}" aria-selected="${s === pg.scenario}"><span aria-hidden="true">${s.icon}</span>${s.code ? `<code>${s.label}</code>` : s.label}</button>`).join('')
    $$('button', scs).forEach((b) =>
      b.addEventListener('click', () => {
        stop()
        pg.scenario = SCENARIOS.find((s) => s.id === b.dataset.id)
        $$('button', scs).forEach((x) => x.setAttribute('aria-selected', String(x === b)))
        buildStage()
        goTo(0, { animate: false })
      }),
    )
    const langs = $('#pg-langs')
    langs.innerHTML = LANGS.map((l) => `<button role="tab" type="button" data-id="${l.id}" aria-selected="${l.id === pg.lang}">${l.label}</button>`).join('')
    $$('button', langs).forEach((b) =>
      b.addEventListener('click', () => {
        pg.lang = b.dataset.id
        $$('button', langs).forEach((x) => x.setAttribute('aria-selected', String(x === b)))
        paintCode(steps()[pg.index])
      }),
    )
  }

  $('#pg-prev').addEventListener('click', () => { stop(); goTo(pg.index - 1, { animate: false }) })
  $('#pg-next').addEventListener('click', () => { stop(); goTo(pg.index + 1) })
  $('#pg-reset').addEventListener('click', () => { stop(); goTo(0, { animate: false }); setPlaying(false) })
  $('#pg-play').addEventListener('click', () => (pg.playing ? stop() : play()))
  $('#pg-deep').addEventListener('change', (e) => {
    stop()
    const current = steps()[pg.index]
    pg.deep = e.target.checked
    const list = steps()
    let i = list.indexOf(current)
    if (i < 0) i = Math.min(pg.index, list.length - 1)
    goTo(i, { animate: false })
  })
  document.addEventListener('keydown', (e) => {
    if (e.target.closest('input, textarea, select, summary')) return
    const r = $('#playground').getBoundingClientRect()
    if (r.bottom < 0 || r.top > window.innerHeight) return
    if (e.key === 'ArrowRight') { stop(); goTo(pg.index + 1) }
    if (e.key === 'ArrowLeft') { stop(); goTo(pg.index - 1, { animate: false }) }
  })

  buildTabs()
  buildStage()
  goTo(0, { animate: false })

  /* =====================================================================
   * FAQ demos
   * ===================================================================== */

  const seg = (name, options, value) =>
    `<div class="seg" role="tablist" data-seg="${name}">${options
      .map(([v, label]) => `<button type="button" role="tab" data-v="${v}" aria-selected="${v === value}">${label}</button>`)
      .join('')}</div>`
  function bindSeg(root, name, onChange) {
    const s = $(`[data-seg="${name}"]`, root)
    $$('button', s).forEach((b) =>
      b.addEventListener('click', () => {
        $$('button', s).forEach((x) => x.setAttribute('aria-selected', String(x === b)))
        onChange(b.dataset.v)
      }),
    )
  }
  const result = (tone, badge, text) => `<div class="result" data-tone="${tone}"><span class="result-badge">${badge}</span><p class="result-text">${text}</p></div>`
  const codeBox = (id) => `<div class="code code-sm">${codeHTML(S.ts[id])}</div>`

  const DEMOS = {
    /* 1. JSON vs stream sizes */
    size(root) {
      const SIZES = [
        [1e3, '1 KB'], [1e5, '100 KB'], [1e6, '1 MB'], [1e7, '10 MB'], [64 * 1048576, '64 MiB'], [1e8, '100 MB'], [1e9, '1 GB'], [1e10, '10 GB'],
      ]
      const LIMIT = 64 * 1048576
      const pos = (b) => Math.max(2, ((Math.log10(b) - 2) / 8) * 100)
      root.innerHTML = `
        <div class="demo-row"><span class="demo-label">File size</span><b class="mono" data-o="size"></b></div>
        <input class="range" type="range" min="0" max="${SIZES.length - 1}" value="2" aria-label="File size">
        <div class="bars">
          <div class="bar-row"><span>JSON in the call</span><div class="bar" data-o="json"><i></i><span class="bar-limit" style="left:${pos(LIMIT)}%"></span><b></b></div></div>
          <div class="bar-row"><span>Stream: the call</span><div class="bar"><i style="width:${pos(150)}%;background:var(--line-strong)"></i><b>~150 B (the ref)</b></div></div>
          <div class="bar-row"><span>Stream: the pipe</span><div class="bar" data-tone="accent" data-o="pipe"><i></i><b></b></div></div>
        </div>
        <div data-o="res"></div>`
      const input = $('input', root)
      const paint = () => {
        const [b, label] = SIZES[+input.value]
        $('[data-o="size"]', root).textContent = label
        const json = $('[data-o="json"]', root)
        const over = b > LIMIT
        json.dataset.tone = over ? 'fail' : ''
        $('i', json).style.width = `${pos(b)}%`
        $('b', json).textContent = over ? `${label} · payload_too_large` : label
        const pipe = $('[data-o="pipe"]', root)
        $('i', pipe).style.width = `${pos(b)}%`
        $('b', pipe).textContent = `${label}, in chunks`
        $('[data-o="res"]', root).innerHTML = over
          ? result('fail', 'JSON fails', `Above 64 MiB the call fails with <code>payload_too_large</code>. With a stream, the call stays the size of a ref, and the receiver only holds the current chunk.`)
          : b >= 1e7
            ? result('warn', 'JSON fits, but it\'s heavy', `It fits under the limit, but the whole file has to be ready before it leaves and sits whole in the receiver's memory. With a stream, the handler starts reading at the first chunk.`)
            : result('muted', 'either works', `For small JSON, use a regular call. A stream pays off for large files, binary data, tokens from a model, or the output of a long job.`)
      }
      input.addEventListener('input', paint)
      paint()
    },

    /* 2. before / after */
    beforeafter(root) {
      root.innerHTML = `
        <div class="demo-row">${seg('v', [['before', '0.26 · before'], ['after', '0.27 · now']], 'after')}</div>
        <div data-o="code"></div>
        <p class="demo-note" data-o="note"></p>`
      const paint = (v) => {
        $('[data-o="code"]', root).innerHTML = codeBox(v)
        $('[data-o="note"]', root).innerHTML = v === 'before'
          ? 'Before: create the channel, connect the file to the <code>writer</code>, pass the <code>readerRef</code>, and read <code>.stream</code> on the other side.'
          : 'Now: the file goes in the payload, and an <code>InputStream</code> arrives on the other side. The field name doesn\'t change.'
      }
      bindSeg(root, 'v', paint)
      paint('after')
    },

    /* 3. classify values */
    classify(root) {
      const ITEMS = [
        ['"app.log"', 'json', 'a string is just text'],
        ['Buffer.from("hi")', 'json', 'a buffer stays JSON'],
        ['{ lines: 6 }', 'json', 'object'],
        ['[1, 2, 3]', 'json', 'array'],
        ['createReadStream("app.log")', 'in', 'Node Readable'],
        ['new Blob(["hi"])', 'in', 'Blob (or File)'],
        ['(async function* () {…})()', 'in', 'async generator object'],
        ['new ReadableStream(…)', 'in', 'Web ReadableStream'],
        ['InputStream.from("hi")', 'in', 'anything wrapped'],
        ['createWriteStream("out.txt")', 'out', 'Node Writable'],
        ['new WritableStream(…)', 'out', 'Web WritableStream'],
      ]
      root.innerHTML = `
        <div class="demo-row" data-o="chips">${ITEMS.map((it, i) => `<button type="button" class="chip" data-i="${i}">${esc(it[0])}</button>`).join('')}</div>
        <div class="bins">
          <div class="bin" data-kind="json"><div class="bin-head"><span>stays JSON</span><small>goes in the call</small></div></div>
          <div class="bin" data-kind="in"><div class="bin-head"><span>becomes InputStream</span><small>the other side reads</small></div></div>
          <div class="bin" data-kind="out"><div class="bin-head"><span>becomes OutputStream</span><small>the other side writes</small></div></div>
        </div>
        <div class="demo-row"><button type="button" class="btn btn-ghost" data-o="reset">Restart</button></div>`
      $$('.chip', root).forEach((c) =>
        c.addEventListener('click', () => {
          if (c.dataset.used === 'true') return
          const [label, kind, note] = ITEMS[+c.dataset.i]
          c.dataset.used = 'true'
          $(`.bin[data-kind="${kind}"]`, root).append(h(`<div class="bin-item">${esc(label)}<em>${note}</em></div>`))
        }),
      )
      $('[data-o="reset"]', root).addEventListener('click', () => {
        $$('.bin-item', root).forEach((n) => n.remove())
        $$('.chip', root).forEach((c) => { c.dataset.used = 'false' })
      })
    },

    /* 4. read an InputStream */
    read(root) {
      const FRAMES = [
        ['binary', 'line 1\\nli'],
        ['binary', 'ne 2\\n'],
        ['text', '{"pct":50}'],
        ['binary', 'line 3\\n'],
      ]
      const M = {
        chunks: { code: 'readChunks', hit: [0, 1, 3], out: ['<Buffer "line 1\\nli">', '<Buffer "ne 2\\n">', '<Buffer "line 3\\n">'], note: 'Chunks arrive as they were written: a line can come split. The message is skipped.' },
        text: { code: 'readText', hit: [0, 1, 3], out: ['"line 1\\nline 2\\nline 3\\n"'], note: 'Reads everything and joins it. Good for small things; the message is skipped. <code>bytes()</code> does the same and returns a <code>Buffer</code>.' },
        lines: { code: 'readLines', hit: [0, 1, 3], out: ['"line 1"', '"line 2"', '"line 3"'], note: 'Joins the chunks and splits on <code>\\n</code>, even when a line came in two chunks.' },
        messages: { code: 'readMessages', hit: [2], out: ['{ pct: 50 }'], note: 'Only the messages, already parsed. The bytes are skipped.' },
        frames: { code: 'readFrames', hit: [0, 1, 2, 3], out: ['{ type: "binary", value: Uint8Array(9) }', '{ type: "binary", value: Uint8Array(5) }', '{ type: "text", value: \'{"pct":50}\' }', '{ type: "binary", value: Uint8Array(7) }'], note: 'Everything, in the order it was sent.' },
      }
      root.innerHTML = `
        <div class="frames">${FRAMES.map(([k, v], i) => `<span class="frame" data-kind="${k}" data-i="${i}"><b>${k === 'text' ? 'message' : 'bytes'}</b>${esc(v)}</span>`).join('')}</div>
        <div class="demo-row">${seg('m', [['chunks', 'for await'], ['text', 'text()'], ['lines', 'lines()'], ['messages', 'messages()'], ['frames', 'frames()']], 'lines')}</div>
        <div data-o="code"></div>
        <div class="demo-out" data-o="out"></div>
        <p class="demo-note" data-o="note"></p>`
      let token = 0
      const paint = async (k) => {
        const m = M[k]
        const my = ++token
        $('[data-o="code"]', root).innerHTML = codeBox(m.code)
        $$('.frame', root).forEach((f, i) => { f.dataset.hit = String(m.hit.includes(i)); f.dataset.dim = String(!m.hit.includes(i)) })
        $('[data-o="note"]', root).innerHTML = m.note
        const out = $('[data-o="out"]', root)
        out.innerHTML = ''
        for (const line of m.out) {
          await wait(160)
          if (my !== token) return
          out.append(h(`<div><span class="muted">›</span> ${esc(line)}</div>`))
        }
      }
      bindSeg(root, 'm', paint)
      paint('lines')
    },

    /* 5. lifecycle */
    lifecycle(root) {
      const s = { type: 'out', end: 'return', use: 'used' }
      const USE = {
        out: [['used', 'wrote to it'], ['unused', 'didn\'t use it'], ['fwd', 'forwarded it']],
        in: [['all', 'read all of it'], ['part', 'read part of it'], ['unused', 'didn\'t read it'], ['fwd', 'forwarded it']],
      }
      root.innerHTML = `
        <div class="select-row"><span class="demo-label">The handler got an</span>${seg('type', [['out', 'OutputStream'], ['in', 'InputStream']], 'out')}</div>
        <div class="select-row"><span class="demo-label">With the stream, it</span><span data-o="use"></span></div>
        <div class="select-row"><span class="demo-label">And at the end it</span>${seg('end', [['return', 'returned'], ['throw', 'threw an error']], 'return')}</div>
        <div data-o="res"></div>`
      const outcome = () => {
        if (s.type === 'out') {
          if (s.use === 'fwd') return ['muted', 'left as is', 'Whoever got the stream in the other call is the one who closes it.']
          if (s.use === 'used') return s.end === 'return'
            ? ['ok', 'closed', 'The reader gets everything and then the end. The caller\'s destination is complete.']
            : ['fail', 'aborted', 'The reader gets what was already written and then <code>STREAM_ABORTED</code>. Never a short file that looks complete.']
          return s.end === 'return'
            ? ['ok', 'closed, empty', 'The reader sees an empty stream right away, and a caller that passed a destination doesn\'t keep waiting. (In a call through an observer, like an HTTP route, this only happens if the handler returns nothing.)']
            : ['muted', 'left as is', 'The SDK doesn\'t touch it. The call fails with the handler\'s error, and the caller\'s SDK closes the destination with that error.']
        }
        if (s.use === 'all') return ['ok', 'finished', 'Nothing to do: the stream already ended.']
        if (s.use === 'part') return ['warn', 'cancelled', 'The SDK cancels the stream and the engine closes the writer. A read that is still running fails with <code>STREAM_CLOSED</code>: what was read never looks like the whole stream.']
        if (s.use === 'unused') return ['muted', 'left as is', 'The SDK doesn\'t touch a stream the handler didn\'t read.']
        return ['muted', 'left as is', 'The function that got the stream is the one that reads it.']
      }
      const paint = () => {
        const [tone, badge, text] = outcome()
        $('[data-o="res"]', root).innerHTML = result(tone, badge, text)
      }
      const paintUse = () => {
        const opts = USE[s.type]
        if (!opts.some(([v]) => v === s.use)) s.use = opts[0][0]
        $('[data-o="use"]', root).innerHTML = seg('use', opts, s.use)
        bindSeg(root, 'use', (v) => { s.use = v; paint() })
      }
      bindSeg(root, 'type', (v) => { s.type = v; paintUse(); paint() })
      bindSeg(root, 'end', (v) => { s.end = v; paint() })
      paintUse()
      paint()
    },

    /* 6. abort in the middle */
    abort(root) {
      const N = 8
      root.innerHTML = `
        <div class="lane"><div class="lane-end">writer<small>out.write()</small></div><div class="lane-track" data-o="track"></div><div class="lane-end">reader<small>for await</small></div></div>
        <div class="demo-row"><button type="button" class="btn" data-o="go">Transfer</button><button type="button" class="btn btn-danger" data-o="abort" disabled>Abort now</button></div>
        <div class="demo-out" data-o="out"><span class="muted">press Transfer</span></div>`
      const track = $('[data-o="track"]', root)
      const out = $('[data-o="out"]', root)
      const go = $('[data-o="go"]', root)
      const ab = $('[data-o="abort"]', root)
      let run = 0
      let aborted = false
      go.addEventListener('click', async () => {
        const my = ++run
        aborted = false
        track.dataset.state = ''
        out.innerHTML = ''
        go.disabled = true
        ab.disabled = false
        let got = 0
        for (let i = 0; i < N; i++) {
          if (my !== run) return
          if (aborted) break
          const c = h('<span class="lane-chunk"></span>')
          track.append(c)
          await c.animate([{ left: '0%' }, { left: 'calc(100% - 14px)' }], { duration: reduced() ? 0 : 520, easing: 'cubic-bezier(0.65,0,0.35,1)' }).finished.catch(() => {})
          c.remove()
          if (my !== run) return
          got++
          out.append(h(`<div><span class="ok">✓</span> chunk ${got}</div>`))
          await wait(180)
        }
        ab.disabled = true
        go.disabled = false
        if (aborted) {
          track.dataset.state = 'aborted'
          out.append(h(`<div class="fail">✕ StreamAbortedError · STREAM_ABORTED</div>`))
          out.append(h(`<div class="muted">  ${got} of ${N} chunks arrived; the loop throws the error instead of finishing.</div>`))
        } else {
          out.append(h(`<div class="acc">end of stream: ${N} of ${N}, complete</div>`))
        }
      })
      ab.addEventListener('click', () => { aborted = true; ab.disabled = true })
    },

    /* 7. backpressure */
    backpressure(root) {
      const CAP = 8
      root.innerHTML = `
        <div class="demo-row"><span class="demo-label">Reader speed</span><b class="mono" data-o="rate"></b></div>
        <input class="range" type="range" min="1" max="12" value="3" aria-label="Reader speed">
        <div class="meter-row"><span>engine buffer</span><span class="slots" data-o="slots">${'<span class="slot"></span>'.repeat(CAP)}</span></div>
        <div class="meter-row"><span>written <b data-o="w">0</b></span><span>read <b data-o="r">0</b></span><span data-o="state"></span></div>
        <p class="demo-note">The writer wants to send 10 chunks per second. When the buffer fills up, its <code>await out.write()</code> waits.</p>`
      const input = $('input', root)
      let buf = 0, w = 0, r = 0, tw = 0, tr = 0, timer = null, last = 0, lastWaiting = null
      const paint = (waiting) => {
        $('[data-o="rate"]', root).textContent = `${input.value} chunks/s`
        $$('.slot', root).forEach((s, i) => { s.dataset.full = String(i < buf) })
        $('[data-o="w"]', root).textContent = w
        $('[data-o="r"]', root).textContent = r
        if (waiting !== lastWaiting) {
          lastWaiting = waiting
          $('[data-o="state"]', root).innerHTML = waiting
            ? '<span class="tag" data-tone="warn">writer waiting: write() hasn\'t resolved yet</span>'
            : '<span class="tag" data-tone="accent">writer writing</span>'
        }
      }
      const tick = (now) => {
        const dt = Math.min(0.1, (now - last) / 1000)
        last = now
        tw += dt * 10
        tr += dt * +input.value
        let waiting = false
        while (tw >= 1) { if (buf < CAP) { buf++; w++; tw-- } else { waiting = true; tw = 1; break } }
        while (tr >= 1) { if (buf > 0) { buf--; r++; tr-- } else { tr = 0; break } }
        if (buf >= CAP) waiting = true
        paint(waiting)
        timer = requestAnimationFrame(tick)
      }
      const start = () => { if (!timer) { last = performance.now(); timer = requestAnimationFrame(tick) } }
      const stopIt = () => { if (timer) cancelAnimationFrame(timer); timer = null }
      input.addEventListener('input', () => paint(buf >= CAP))
      root.closest('details').addEventListener('toggle', (e) => (e.target.open ? start() : stopIt()))
      paint(false)
    },

    /* 8. read twice / tee */
    tee(root) {
      let mode = 'plain', reads = 0
      root.innerHTML = `
        <div class="demo-row">${seg('t', [['plain', 'without tee()'], ['tee', 'with tee()']], 'plain')}</div>
        <div data-o="code"></div>
        <div class="demo-row" data-o="btns"></div>
        <div class="demo-grid2"><div class="demo-out" data-o="o1"></div><div class="demo-out" data-o="o2"></div></div>`
      const o1 = $('[data-o="o1"]', root), o2 = $('[data-o="o2"]', root)
      const paint = () => {
        reads = 0
        $('[data-o="code"]', root).innerHTML = codeBox(mode === 'plain' ? 'twice' : 'tee')
        o1.innerHTML = `<span class="muted">${mode === 'plain' ? 'read 1' : 'copy 1 (forHash)'}</span>`
        o2.innerHTML = `<span class="muted">${mode === 'plain' ? 'read 2' : 'copy 2 (forStore)'}</span>`
        const btns = $('[data-o="btns"]', root)
        btns.innerHTML = mode === 'plain'
          ? '<button type="button" class="btn" data-o="r">Read</button>'
          : '<button type="button" class="btn" data-o="r">Read both copies, together</button>'
        $('[data-o="r"]', root).addEventListener('click', async () => {
          if (mode === 'plain') {
            reads++
            if (reads === 1) {
              for (const l of ['line 1', 'line 2', 'line 3']) { await wait(140); o1.append(h(`<div><span class="ok">✓</span> ${l}</div>`)) }
              $('[data-o="r"]', root).textContent = 'Read again'
            } else if (reads === 2) {
              o2.append(h('<div class="fail">✕ STREAM_IN_USE</div>'))
              o2.append(h('<div class="muted">  the stream already has a reader</div>'))
              $('[data-o="r"]', root).disabled = true
            }
          } else {
            $('[data-o="r"]', root).disabled = true
            for (const l of ['line 1', 'line 2', 'line 3']) {
              await wait(160)
              o1.append(h(`<div><span class="ok">✓</span> ${l}</div>`))
              o2.append(h(`<div><span class="ok">✓</span> ${l}</div>`))
            }
          }
        })
      }
      bindSeg(root, 't', (v) => { mode = v; paint() })
      paint()
    },

    /* 9. forward without reading */
    forward(root) {
      let mode = 'ref'
      root.innerHTML = `
        <div class="demo-row">${seg('f', [['ref', 'B doesn\'t read'], ['pump', 'B creates a new stream']], 'ref')}</div>
        <div data-o="code"></div>
        <div class="fwd" data-o="fwd">
          <div class="lane-end fwd-a">A<small>sends app.log</small></div>
          <div class="lane-end fwd-b">B<small>uploads::receive</small></div>
          <div class="lane-end fwd-c">C<small>count-errors</small></div>
          <div class="lane-track fwd-ab"></div>
          <div class="lane-track fwd-bc"></div>
          <div class="lane-track fwd-direct" data-o="direct"></div>
          <span class="fwd-label" data-o="label"></span>
        </div>
        <div class="demo-row"><button type="button" class="btn" data-o="go">Send</button><span class="demo-note" data-o="count"></span></div>`
      const fwd = $('[data-o="fwd"]', root)
      let run = 0
      const clear = () => $$('.lane-chunk, .ref-pill', fwd).forEach((n) => n.remove())
      const paint = () => {
        $('[data-o="code"]', root).innerHTML = codeBox(mode === 'ref' ? 'forward' : 'forwardRead')
        fwd.dataset.mode = mode
        $('[data-o="label"]', root).textContent = mode === 'ref' ? 'a single pipe: A → C' : 'two pipes: A → B and B → C'
        $('[data-o="count"]', root).textContent = ''
      }
      const move = (el, frames, dur) =>
        el.animate(frames, { duration: reduced() ? 0 : dur, easing: 'cubic-bezier(0.65,0,0.35,1)', fill: 'forwards' }).finished.catch(() => {})
      $('[data-o="go"]', root).addEventListener('click', async () => {
        const my = ++run
        clear()
        const count = $('[data-o="count"]', root)
        count.textContent = ''
        // the call, with the ref, goes A → B → C
        const pill = h('<span class="tag ref-pill" data-tone="warn">ref</span>')
        fwd.append(pill)
        await move(pill, [{ left: '28%' }, { left: 'calc(50% - 18px)' }], 600)
        if (my !== run) return
        await move(pill, [{ left: 'calc(50% - 18px)' }, { left: 'calc(72% - 36px)' }], 600)
        pill.remove()
        if (my !== run) return
        const flights = []
        for (let i = 0; i < 6; i++) {
          if (my !== run) return
          const c = h('<span class="lane-chunk"></span>')
          fwd.append(c)
          if (mode === 'ref') {
            c.style.top = '104px'
            flights.push(move(c, [{ left: '14%' }, { left: 'calc(86% - 14px)' }], 1000).then(() => c.remove()))
          } else {
            c.style.top = '29px'
            flights.push(move(c, [{ left: '28%' }, { left: 'calc(50% - 7px)', offset: 0.45 }, { left: 'calc(50% - 7px)', offset: 0.55 }, { left: 'calc(72% - 14px)' }], 1600).then(() => c.remove()))
          }
          await wait(260)
        }
        await Promise.all(flights)
        if (my !== run) return
        count.innerHTML = mode === 'ref'
          ? 'Bytes that went through B: <strong>none</strong>. Only the ref did.'
          : 'Bytes that went through B: <strong>all of them</strong>. B pumps the stream it created.'
      })
      bindSeg(root, 'f', (v) => { mode = v; run++; clear(); paint() })
      paint()
    },

    /* 10. sink vs pipe */
    sinkpipe(root) {
      root.innerHTML = `
        <p class="demo-note">The handler answers early (<code>keepOpen()</code> and <code>return { started: true }</code>) and keeps writing. The bar is the stream.</p>
        <div class="timeline">
          <div class="tl-row"><span>Destination (file)</span><div class="tl" data-o="sink"><i></i></div></div>
          <div class="tl-row"><span><code>pipe()</code></span><div class="tl" data-o="pipe"><i class="acc"></i></div></div>
        </div>
        <div class="demo-row"><button type="button" class="btn" data-o="go">Run</button></div>
        <div class="demo-grid2">
          <div class="demo-box"><h5>Destination</h5><p class="demo-note">The call waits for the stream to finish. Messages (<code>send</code>) are dropped.</p></div>
          <div class="demo-box"><h5>pipe()</h5><p class="demo-note">The call resolves when the handler returns. Messages arrive on the <code>input</code>.</p></div>
        </div>`
      const sink = $('[data-o="sink"]', root), pipe = $('[data-o="pipe"]', root)
      let run = 0
      const mark = (tl, at, label, end = false) => tl.append(h(`<span class="tl-mark${end ? ' end' : ''}" data-label="${label}" style="left:${at}"></span>`))
      $('[data-o="go"]', root).addEventListener('click', async () => {
        const my = ++run
        $$('.tl-mark', root).forEach((n) => n.remove())
        const bars = [$('i', sink), $('i', pipe)]
        bars.forEach((b) => { b.style.width = '0' })
        const T = reduced() ? 0 : 3200
        const t0 = performance.now()
        let marked = false
        await new Promise((resolve) => {
          const step = (now) => {
            if (my !== run) return resolve()
            const p = T ? Math.min(1, (now - t0) / T) : 1
            bars.forEach((b) => { b.style.width = `${p * 100}%` })
            if (!marked && p >= 0.2) {
              marked = true
              mark(sink, '20%', 'handler returns')
              mark(pipe, '20%', 'handler returns → the call resolves')
            }
            if (p < 1) requestAnimationFrame(step)
            else resolve()
          }
          requestAnimationFrame(step)
        })
        if (my !== run) return
        mark(sink, 'calc(100% - 2px)', 'the call resolves', true)
      })
    },

    /* 11. queue */
    queue(root) {
      const R = {
        sync: ['ok', 'works', 'The call waits for the result; the stream flows while it runs.', 'callSync'],
        void: ['ok', 'works', 'The call is live; only the result is discarded.', 'callVoid'],
        enqueue: ['fail', 'STREAM_NOT_ALLOWED', 'Rejected before it leaves: the job can run after the pipe has expired, and a retry couldn\'t read the stream again.', 'enqueue'],
      }
      root.innerHTML = `
        <div class="demo-row">${seg('a', [['sync', 'default (sync)'], ['void', 'Void()'], ['enqueue', 'Enqueue()']], 'enqueue')}</div>
        <div data-o="code"></div><div data-o="res"></div>`
      const paint = (v) => {
        const [tone, badge, text, code] = R[v]
        $('[data-o="code"]', root).innerHTML = codeBox(code)
        $('[data-o="res"]', root).innerHTML = result(tone, badge, text)
      }
      bindSeg(root, 'a', paint)
      paint('enqueue')
    },

    /* 12. observers see a StreamInfo */
    observe(root) {
      root.innerHTML = `
        ${codeBox('observe')}
        <div class="demo-row">${seg('o', [['handler', 'logs::count-errors gets'], ['observer', 'audit::uploads gets']], 'observer')}</div>
        <div class="demo-out" data-o="out"></div>
        <p class="demo-note" data-o="note"></p>`
      const paint = (v) => {
        $('[data-o="out"]', root).innerHTML = v === 'handler'
          ? `{ <span class="muted">log:</span> <span class="acc">InputStream</span> }`
          : `{\n  <span class="muted">function_id:</span> "logs::count-errors",\n  <span class="muted">input:</span> {\n    <span class="muted">log:</span> { <span class="muted">stream:</span> true, <span class="muted">direction:</span> "read", <span class="muted">id:</span> "7f3a…" }\n  },\n  …\n}`
        $('[data-o="note"]', root).innerHTML = v === 'handler'
          ? 'A live stream, ready to read.'
          : 'A <code>StreamInfo</code>: it says there was a stream and which one (the pipe <code>id</code>), without the access key. It can\'t be read.'
      }
      bindSeg(root, 'o', paint)
      paint('observer')
    },

    /* 13. browser file */
    browser(root) {
      root.innerHTML = `
        <label class="file-pick"><input type="file"><span>▤ Pick a file</span></label>
        <div data-o="code">${codeBox('browser')}</div>
        <div class="demo-out" data-o="out"><span class="muted">no file yet</span></div>`
      const input = $('input', root)
      const out = $('[data-o="out"]', root)
      const fmt = (n) => (n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1048576).toFixed(1)} MB`)
      input.addEventListener('change', async () => {
        const file = input.files && input.files[0]
        if (!file) return
        out.innerHTML = `<div><span class="muted">payload:</span> { <span class="muted">log:</span> File("${esc(file.name)}", ${fmt(file.size)}) }</div><div><span class="muted">the handler gets:</span> <span class="acc">log: InputStream</span></div><div class="muted">(read locally, only in your browser, with the same loop)</div><div data-o="live"></div>`
        const live = $('[data-o="live"]', out)
        let bytes = 0, chunks = 0, lines = 0
        try {
          const reader = file.stream().getReader()
          for (;;) {
            const { value, done } = await reader.read()
            if (done) break
            chunks++
            bytes += value.byteLength
            for (let i = 0; i < value.length; i++) if (value[i] === 10) lines++
            live.innerHTML = `chunks: <b>${chunks}</b> · bytes: <b>${fmt(bytes)}</b> · line breaks: <b>${lines}</b>`
            if (chunks % 8 === 0) await new Promise((r) => setTimeout(r))
          }
          live.innerHTML += ' <span class="ok">✓ done</span>'
        } catch (e) {
          live.innerHTML = `<span class="fail">couldn't read it: ${esc(e.message)}</span>`
        }
      })
    },

    /* 14. compatibility */
    compat(root) {
      const s = { where: 'payload', who: 'new' }
      const T = {
        payload: { new: ['accent', 'InputStream / OutputStream', 'The 0.27 SDK turns the ref into a stream.'], oldDyn: ['warn', 'ChannelReader / ChannelWriter', 'The 0.26 TypeScript, Python, or browser SDK gives you the old channel API.'], oldTyped: ['muted', 'the ref as JSON', 'The 0.26 Go or Rust SDK gives you the ref JSON, as before.'] },
        result: { new: ['accent', 'InputStream / OutputStream', 'New in 0.27: refs in the result also become streams.'], oldDyn: ['muted', 'the ref as JSON', 'In 0.26, a call\'s result didn\'t convert refs.'], oldTyped: ['muted', 'the ref as JSON', 'In 0.26, a call\'s result didn\'t convert refs.'] },
      }
      root.innerHTML = `
        <div class="select-row"><span class="demo-label">The stream is</span>${seg('where', [['payload', 'in the payload'], ['result', 'in the result']], 'payload')}</div>
        <div class="select-row"><span class="demo-label">The receiver uses</span>${seg('who', [['new', 'SDK 0.27'], ['oldDyn', '0.26 · TS, Python, browser'], ['oldTyped', '0.26 · Go, Rust']], 'new')}</div>
        <div data-o="res"></div>
        <p class="demo-note">The sender can be on 0.26 or 0.27: the ref is the same JSON. The engine can be 0.26 or 0.27.</p>`
      const paint = () => {
        const [tone, badge, text] = T[s.where][s.who]
        $('[data-o="res"]', root).innerHTML = result(tone, badge, text)
      }
      bindSeg(root, 'where', (v) => { s.where = v; paint() })
      bindSeg(root, 'who', (v) => { s.who = v; paint() })
      paint()
    },

    /* 15. error codes */
    errors(root) {
      const E = {
        STREAM_ABORTED: ['fail', 'The writer failed, called <code>abort()</code>, or went down. You got part of the data. <strong>Treat it as an incomplete transfer</strong> and discard the partial data.', 'TS and Python: <code>StreamAbortedError</code> · Rust: <code>Error::StreamAborted</code> · Go: <code>*iii.InvocationError</code> with <code>iii.CodeStreamAborted</code>'],
        STREAM_CLOSED: ['warn', 'A write or a <code>send</code> after close, after abort, or after the reader left. Or a read of a cancelled stream: by <code>cancel()</code>, by leaving the loop, or because the handler returned.', 'TS: <code>InvocationError</code> with <code>code</code> · Python: <code>StreamClosedError</code> · Rust: <code>Error::StreamClosed</code> · Go: <code>iii.CodeStreamClosed</code>'],
        STREAM_IN_USE: ['warn', 'A second read of the same <code>InputStream</code>, the same stream twice in one payload, or forwarding a stream that was already read to the end or already sent.', 'TS: <code>InvocationError</code> with <code>code</code> · Python: <code>StreamInUseError</code> · Rust: <code>Error::StreamInUse</code> · Go: <code>iii.CodeStreamInUse</code>'],
        STREAM_NOT_ALLOWED: ['fail', 'A stream in a call with <code>Enqueue</code>. The call is rejected before it leaves. Store the data first and enqueue the key.', 'The same code in every SDK.'],
        TIMEOUT: ['muted', 'The call timeout also covers the copy into destinations. If it expires before a destination finishes, the SDK aborts the open destinations.', 'For a destination that takes a while to fill, raise the call timeout.'],
      }
      root.innerHTML = `<div class="demo-row">${Object.keys(E).map((k, i) => `<button type="button" class="chip" data-k="${k}">${k}</button>`).join('')}</div><div data-o="res"></div>`
      const paint = (k) => {
        const [tone, text, names] = E[k]
        $('[data-o="res"]', root).innerHTML = result(tone, k, `${text}<br><span class="muted" style="font-size:13px">${names}</span>`)
        $$('.chip', root).forEach((c) => { c.style.borderColor = c.dataset.k === k ? 'var(--foreground)' : '' })
      }
      $$('.chip', root).forEach((c) => c.addEventListener('click', () => paint(c.dataset.k)))
      paint('STREAM_ABORTED')
    },
  }

  $$('[data-demo]').forEach((el) => {
    const fn = DEMOS[el.dataset.demo]
    if (fn) fn(el)
  })
  fillSnippets($('#faq'))

  // Opening a question from a link (#q-...) opens it.
  const openHash = () => {
    const t = location.hash && document.getElementById(location.hash.slice(1))
    if (t && t.tagName === 'DETAILS') t.open = true
  }
  window.addEventListener('hashchange', openHash)
  openHash()
})()
