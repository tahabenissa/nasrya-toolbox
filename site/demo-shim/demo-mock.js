// Winsploit web view — demo mode.
//
// The real page (index.html, app.css, app.js from src/web) runs unchanged; this
// file loads first and stands in for the terminal: it replaces EventSource
// ('events') and fetch ('api/action', 'api/log') with a simulated session that
// speaks the same protocol as src/web.rs (hello / state / out / toast frames).
// Nothing is executed and nothing leaves the tab.
'use strict';
(() => {
  const D = window.__WS_DEMO__; // catalog, written by scripts/sync.mjs
  const SLOTS = D.slots;

  // the demo is dark unless the visitor picked a theme before
  try { if (!localStorage.getItem('ws-theme')) localStorage.setItem('ws-theme', 'dark'); } catch { /* private mode */ }

  // ------------------------------------------------------------- session

  const tools = D.tools;
  const idx = (name) => tools.findIndex((t) => t.name === name);
  const status = tools.map((t, i) => {
    if (t.name === 'httpx') return 'conflict';
    if (['ffuf', 'katana', 'feroxbuster', 'arjun', 'amass', 'john', 'ghidra', 'volatility3', 'sherlock'].includes(t.name)) return 'missing';
    return i % 9 === 7 ? 'missing' : 'installed';
  });
  const conflicts = tools.map((t) => (t.name === 'httpx' ? 'C:\\Users\\you\\AppData\\Roaming\\Python\\Python312\\Scripts\\httpx.exe' : null));
  let nextSid = 0;
  const S = {
    terminals: [null, null, null, null, null],
    installing: null,
    detecting: false,
    status_line: 'ready',
    history: {},
    lines: new Map(), // sid -> lines
    timers: new Map(), // sid -> interval
    stdinWait: new Map(), // sid -> fn(line)
  };

  function newSession(pos, title, subtitle) {
    const sid = nextSid++;
    S.terminals[pos] = { pos, sid, title, subtitle, state: 'running', code: null, failed: false, pid: 4000 + sid * 137, stdin: true, log: true };
    S.lines.set(sid, []);
    return sid;
  }
  // the console (sid 0) is always open
  newSession(0, 'console', 'installs · hash ids · messages');
  Object.assign(S.terminals[0], { state: 'idle', pid: null, stdin: false });

  // ------------------------------------------------------------- events out

  const sources = new Set();
  function emit(type, data) {
    for (const es of sources) es._emit(type, data);
  }
  // challenge workspaces (C in the terminal); the page can switch between them
  const CHALLENGES = [
    { id: 'picoCTF-2026/rev-crackme1', event: 'picoCTF-2026', name: 'rev-crackme1', target: '', flag_format: '', files: ['crackme1.exe'] },
    { id: 'ACME-CTF/web-login', event: 'ACME-CTF', name: 'web-login', target: 'http://10.10.14.7:8080', flag_format: 'ACME', files: [] },
  ];
  S.challenge = CHALLENGES[1];

  // recipes filled from the active challenge, like recipe::fill_partial:
  // each known value becomes one quoted argument, unknown ones stay as {gaps}
  const quote = (v) => `"${v.trim().replace(/\\+$/, '').replaceAll('"', '\\"')}"`;
  function recipesJson() {
    const c = S.challenge;
    const dir = c ? `C:\\Users\\you\\ctf\\${c.event}\\${c.name}` : null;
    const values = {
      target: c && c.target ? c.target : null,
      file: c && c.files.length === 1 ? `${dir}\\files\\${c.files[0]}` : null,
      out: dir ? `${dir}\\out` : null,
      wordlist: 'C:\\Users\\you\\scoop\\apps\\seclists-lite\\current\\common.txt',
    };
    const out = {};
    for (const t of tools) {
      if (!t.recipes || !t.recipes.length) continue;
      out[t.name] = t.recipes.map((r) => ({
        name: r.name,
        args: r.args.replace(/\{(target|file|out|wordlist)\}/g, (m, k) => (values[k] ? quote(values[k]) : m)),
      }));
    }
    return out;
  }

  function stateJson() {
    return {
      status,
      conflicts,
      terminals: S.terminals,
      installing: S.installing,
      detecting: S.detecting,
      status_line: S.status_line,
      history: S.history,
      viewers: 1,
      lan: false,
      challenge: S.challenge,
      challenges: CHALLENGES.map((c) => ({ id: c.id, title: `${c.event} / ${c.name}` })),
      recipes: recipesJson(),
      flags: [],
    };
  }
  let stateQueued = false;
  function pushState() {
    if (stateQueued) return;
    stateQueued = true;
    queueMicrotask(() => {
      stateQueued = false;
      emit('state', stateJson());
    });
  }
  function out(sid, ...lines) {
    S.lines.get(sid)?.push(...lines);
    emit('out', { sid, reset: false, lines });
  }
  function toast(kind, text) {
    emit('toast', { kind, text });
  }

  class DemoEventSource {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSED = 2;
    constructor(url) {
      this.url = url;
      this.readyState = 0;
      this._l = {};
      this.onopen = null;
      this.onerror = null;
      sources.add(this);
      setTimeout(() => {
        this.readyState = 1;
        this.onopen?.({});
        this._emit('hello', D.hello);
        this._emit('you', { role: 'owner' }); // newer pages: this tab is the session owner
        this._emit('state', stateJson());
        for (const t of S.terminals) if (t) this._emit('out', { sid: t.sid, reset: true, lines: S.lines.get(t.sid) });
      }, 250);
    }
    addEventListener(type, fn) { (this._l[type] ||= []).push(fn); }
    removeEventListener(type, fn) { this._l[type] = (this._l[type] || []).filter((f) => f !== fn); }
    close() { this.readyState = 2; sources.delete(this); }
    _emit(type, data) { for (const fn of this._l[type] || []) fn({ data: JSON.stringify(data) }); }
  }
  window.EventSource = DemoEventSource;

  // ------------------------------------------------------------- canned output

  const SEC = 'C:\\Users\\you\\scoop\\apps\\seclists-lite\\current';
  const OUTPUT = {
    nmap: () => [
      'Starting Nmap 7.95 ( https://nmap.org )',
      'Nmap scan report for scanme.nmap.org (45.33.32.156)',
      'Host is up (0.16s latency).',
      'Not shown: 996 closed tcp ports (reset)',
      'PORT      STATE SERVICE    VERSION',
      '22/tcp    open  ssh        OpenSSH 6.6.1p1 Ubuntu 2ubuntu2.13 (Ubuntu Linux; protocol 2.0)',
      '80/tcp    open  http       Apache httpd 2.4.7 ((Ubuntu))',
      '9929/tcp  open  nping-echo Nping echo',
      '31337/tcp open  tcpwrapped',
      'Service Info: OS: Linux; CPE: cpe:/o:linux:linux_kernel',
      '',
      'Service detection performed. Please report any incorrect results at https://nmap.org/submit/ .',
      'Nmap done: 1 IP address (1 host up) scanned in 14.82 seconds',
    ],
    ffuf: () => [
      '',
      ' :: Method           : GET',
      ' :: URL              : https://example.com/FUZZ',
      ` :: Wordlist         : FUZZ: ${SEC}\\common.txt`,
      ' :: Follow redirects : false',
      ' :: Threads          : 40',
      ' :: Matcher          : Response status: 200-299,301,302,307,401,403,405,500',
      '________________________________________________',
      '',
      'admin                   [Status: 301, Size: 169, Words: 5, Lines: 8, Duration: 41ms]',
      'api                     [Status: 401, Size: 52, Words: 4, Lines: 1, Duration: 38ms]',
      'assets                  [Status: 301, Size: 169, Words: 5, Lines: 8, Duration: 37ms]',
      'backup                  [Status: 403, Size: 153, Words: 3, Lines: 7, Duration: 44ms]',
      'index.html              [Status: 200, Size: 1256, Words: 298, Lines: 47, Duration: 39ms]',
      'login                   [Status: 200, Size: 4127, Words: 811, Lines: 96, Duration: 52ms]',
      'robots.txt              [Status: 200, Size: 64, Words: 6, Lines: 4, Duration: 35ms]',
      ':: Progress: [4750/4750] :: Job [1/1] :: 312 req/sec :: Duration: [0:00:15] :: Errors: 0 ::',
    ],
    gobuster: () => [
      '===============================================================',
      'Gobuster v3.6',
      '===============================================================',
      '[+] Url:                     https://example.com',
      '[+] Method:                  GET',
      '[+] Threads:                 10',
      `[+] Wordlist:                ${SEC}\\common.txt`,
      '===============================================================',
      'Starting gobuster in directory enumeration mode',
      '===============================================================',
      '/admin                (Status: 301) [Size: 169] [--> https://example.com/admin/]',
      '/assets               (Status: 301) [Size: 169] [--> https://example.com/assets/]',
      '/index.html           (Status: 200) [Size: 1256]',
      '/login                (Status: 200) [Size: 4127]',
      '/robots.txt           (Status: 200) [Size: 64]',
      'Progress: 4750 / 4751 (99.98%)',
      '===============================================================',
      'Finished',
      '===============================================================',
    ],
    subfinder: () => [
      '[INF] Current subfinder version v2.6.6 (latest)',
      '[INF] Loading provider config from C:\\Users\\you\\AppData\\Roaming\\subfinder\\provider-config.yaml',
      '[INF] Enumerating subdomains for example.com',
      'www.example.com',
      'mail.example.com',
      'dev.example.com',
      'api.example.com',
      'staging.example.com',
      '[INF] Found 5 subdomains for example.com in 3 seconds 211 milliseconds',
    ],
    nuclei: () => [
      '[INF] nuclei-templates are up to date',
      '[INF] Templates loaded for current scan: 7712',
      '[INF] Targets loaded for current scan: 1',
      '[tech-detect:nginx] [http] [info] https://example.com',
      '[http-missing-security-headers:strict-transport-security] [http] [info] https://example.com',
      '[http-missing-security-headers:content-security-policy] [http] [info] https://example.com',
      '[INF] Scan completed in 41.27s. 3 matches found.',
    ],
    hashcat: () => [
      'hashcat (v6.2.6) starting',
      '',
      'Hashes: 1 digests; 1 unique digests, 1 unique salts',
      'Dictionary cache hit:',
      '* Filename..: rockyou-lite.txt',
      '',
      '5f4dcc3b5aa765d61d8327deb882cf99:password',
      '',
      'Session..........: hashcat',
      'Status...........: Cracked',
      'Hash.Mode........: 0 (MD5)',
      'Speed.#1.........:  4108.2 MH/s',
    ],
    sqlmap: () => [
      '        ___',
      '       __H__',
      ' ___ ___[.]_____ ___ ___  {1.8.6#stable}',
      '|_ -| . [)]     | .\'| . |',
      '|___|_  ["]_|_|_|__,|  _|',
      '      |_|V...       |_|   https://sqlmap.org',
      '',
      '[!] legal disclaimer: Usage of sqlmap for attacking targets without prior mutual consent is illegal.',
      '',
      'sqlmap 1.8.6#stable',
    ],
  };
  const generic = (t, args) => [
    `${t.name}: ${t.description}`,
    `(demo) this is where ${t.run_cmd || t.name}'s live output would stream`,
    args ? `(demo) arguments: ${args}` : '(demo) no arguments',
  ];

  // ------------------------------------------------------------- actions

  function freeSlot() {
    for (let p = 1; p <= SLOTS; p++) {
      const t = S.terminals[p];
      if (!t || t.state !== 'running') return p;
    }
    return null;
  }
  function stopTimer(sid) {
    clearInterval(S.timers.get(sid));
    S.timers.delete(sid);
    S.stdinWait.delete(sid);
  }
  function finish(pos, code) {
    const t = S.terminals[pos];
    if (!t || t.state !== 'running') return;
    stopTimer(t.sid);
    out(t.sid, `── process exited (code ${code}) ──`);
    Object.assign(t, { state: 'exited', code, failed: code !== 0, stdin: false });
    S.status_line = `${t.title} finished`;
    toast(code === 0 ? 'ok' : 'err', `${t.title} finished (exit ${code})`);
    pushState();
  }
  function streamLines(pos, sid, lines, each, then) {
    let i = 0;
    const iv = setInterval(() => {
      if (S.terminals[pos]?.sid !== sid) return stopTimer(sid);
      if (i < lines.length) out(sid, lines[i++]);
      else {
        stopTimer(sid);
        then();
      }
    }, each);
    S.timers.set(sid, iv);
  }

  function run({ tool, args, slot }) {
    const i = idx(tool);
    if (i < 0) return [404, 'no such tool'];
    const t = tools[i];
    if (status[i] === 'missing') {
      S.status_line = `${t.name} is not installed — press i to install it`;
      pushState();
      return [409, `${t.name} is not installed: install it first`];
    }
    if (status[i] === 'conflict') {
      out(0, `[err] the '${t.name}' on PATH (${conflicts[i]}) isn't ${t.name} — a different program with the same name comes first. Not running it.`,
        '[err] fix: press i to install the right one, or move its folder earlier in PATH (or uninstall the other program), then press r.');
      S.status_line = `${t.name}: name conflict, not run (see console)`;
      pushState();
      return [200, ''];
    }
    if (t.launch === 'dataset') {
      out(0, `$ ${t.name} — C:\\Users\\you\\scoop\\apps\\${t.name}\\current`,
        '   common.txt                                    36.4 KB',
        '   raft-medium-directories.txt                  250.2 KB',
        '   xato-net-10-million-passwords-10000.txt       75.3 KB',
        ':: folder path copied to the clipboard');
      pushState();
      return [200, ''];
    }
    if (t.launch === 'gui') {
      out(0, `$ ${t.run_cmd} ${args}`.trim(), `:: ${t.name} started in its own window (demo: nothing was started)`);
      toast('ok', `${t.name} launched`);
      pushState();
      return [200, ''];
    }
    const pos = slot && slot >= 1 && slot <= SLOTS ? slot : freeSlot();
    if (!pos) {
      toast('err', `all ${SLOTS} terminals are busy: pick one to replace`);
      return [200, ''];
    }
    const old = S.terminals[pos];
    if (old) {
      stopTimer(old.sid);
      S.lines.delete(old.sid);
    }
    const sid = newSession(pos, t.name, t.description);
    const hist = (S.history[t.name] ||= []);
    if (args && hist[hist.length - 1] !== args) hist.push(args);
    out(sid, ':: started from the browser');
    if (t.note) out(sid, `:: note: ${t.note}`);
    out(sid, `$ ${t.run_cmd} ${args}`.trim(), `:: log: C:\\Users\\you\\AppData\\Local\\winsploit\\logs\\${new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)}-${t.name}.log`);
    if (t.launch === 'web') out(sid, `:: opening ${t.url} in your browser`);
    S.status_line = `running ${t.name} in terminal ${pos} (pid ${S.terminals[pos].pid})`;
    pushState();
    const lines = (OUTPUT[t.name] || (() => generic(t, args)))();
    if (t.name === 'sqlmap') {
      // an interactive prompt: the visitor can answer it with the input box
      streamLines(pos, sid, lines, 90, () => {
        out(sid, '[?] do you want to continue testing https://example.com? [Y/n] ');
        S.status_line = 'sqlmap is waiting for input · type a line below the output';
        pushState();
        S.stdinWait.set(sid, (line) => {
          out(sid, line.trim().toLowerCase().startsWith('n') ? '[*] ending @ user request' : '[*] testing connection to the target URL', '[*] (demo) that\'s where the real run would continue');
          setTimeout(() => finish(pos, 0), 700);
        });
      });
    } else {
      streamLines(pos, sid, lines, t.name === 'nmap' ? 420 : 160, () => setTimeout(() => finish(pos, 0), 300));
    }
    return [200, ''];
  }

  function install({ tool }) {
    const i = idx(tool);
    if (i < 0) return [404, 'no such tool'];
    const t = tools[i];
    if (S.installing) return [409, `already installing ${S.installing}`];
    if (status[i] === 'installed') {
      out(0, `:: ${t.name} is already installed`);
      pushState();
      return [200, ''];
    }
    S.installing = t.name;
    const step = t.install === 'pipx'
      ? `pipx install ${t.package}`
      : `${t.bucket && t.bucket !== 'main' ? `scoop bucket add ${t.bucket} && ` : ''}scoop install ${t.bucket && t.bucket !== 'main' ? `${t.bucket}/` : ''}${t.package}`;
    out(0, `:: install of ${t.name} started from the browser`, `$ ${step}`);
    const lines = t.install === 'pipx'
      ? [`creating virtual environment...`, `installing ${t.package}...`, `  installed package ${t.package}, installed using Python 3.12.7`, `  These apps are now globally available`, `    - ${t.exe}.exe`, 'done!']
      : [`Installing '${t.package}' [64bit] from '${t.bucket || 'main'}' bucket`, `${t.package}.zip (4.1 MB) [===============================] 100%`, `Checking hash of ${t.package}.zip ... ok.`, `Extracting ${t.package}.zip ... done.`, `Linking ~\\scoop\\apps\\${t.package}\\current`, `Creating shim for '${t.exe}'.`, `'${t.package}' was installed successfully!`];
    S.status_line = `installing ${t.name}…`;
    pushState();
    let k = 0;
    const iv = setInterval(() => {
      if (k < lines.length) return out(0, lines[k++]);
      clearInterval(iv);
      out(0, '── process exited (code 0) ──');
      status[i] = 'installed';
      conflicts[i] = null;
      S.installing = null;
      S.status_line = `${t.name} installed · PATH refreshed`;
      toast('ok', `${t.name} installed`);
      pushState();
    }, 280);
    return [200, ''];
  }

  // a tiny version of src/hashid.rs: enough for the common shapes
  function identify(h) {
    const s = h.trim();
    const hex = /^[0-9a-f]+$/i.test(s);
    const r = (name, m, j) => `  ${name.padEnd(22)}  ${m != null ? `hashcat -m ${m}`.padEnd(16) : 'hashcat: n/a'.padEnd(16)} ${j ? `john --format=${j}` : ''}`.trimEnd();
    if (/^\$2[aby]\$\d\d\$/.test(s)) return [r('bcrypt', 3200, 'bcrypt')];
    if (s.startsWith('$1$')) return [r('md5crypt (Unix MD5)', 500, 'md5crypt')];
    if (s.startsWith('$6$')) return [r('sha512crypt', 1800, 'sha512crypt')];
    if (s.startsWith('$5$')) return [r('sha256crypt', 7400, 'sha256crypt')];
    if (s.startsWith('$argon2')) return [r('Argon2', 34000, null)];
    if (hex && s.length === 32) return ['3 candidates, most likely first:', r('MD5', 0, 'raw-md5'), r('NTLM', 1000, 'nt'), r('MD4', 900, 'raw-md4')];
    if (hex && s.length === 40) return ['2 candidates, most likely first:', r('SHA-1', 100, 'raw-sha1'), r('RIPEMD-160', 6000, 'ripemd-160')];
    if (hex && s.length === 64) return ['3 candidates, most likely first:', r('SHA-256', 1400, 'raw-sha256'), r('SHA3-256', 17400, null), r('Keccak-256', 17800, null)];
    if (hex && s.length === 128) return ['3 candidates, most likely first:', r('SHA-512', 1700, 'raw-sha512'), r('SHA3-512', 17600, null), r('Whirlpool', 6100, 'whirlpool')];
    return [`no match for "${s}" — not a hash format winsploit knows`];
  }

  function handle(a) {
    switch (a.type) {
      case 'run': return run(a);
      case 'install': return install(a);
      case 'cancel': {
        const t = S.terminals[a.term];
        if (!t || t.state !== 'running') return [409, 'nothing is running there'];
        stopTimer(t.sid);
        out(t.sid, '── process killed ──');
        Object.assign(t, { state: 'killed', stdin: false });
        S.status_line = `stopped ${t.title}`;
        pushState();
        return [200, ''];
      }
      case 'stdin': {
        const t = S.terminals[a.term];
        if (!t || t.state !== 'running') return [409, 'the tool\'s input is closed'];
        out(t.sid, `> ${a.line}`);
        const wait = S.stdinWait.get(t.sid);
        if (wait) {
          S.stdinWait.delete(t.sid);
          wait(a.line);
        }
        return [200, ''];
      }
      case 'eof': {
        const t = S.terminals[a.term];
        if (!t) return [409, 'no terminal there'];
        out(t.sid, ':: closed the tool\'s input (EOF)');
        t.stdin = false;
        pushState();
        return [200, ''];
      }
      case 'close': {
        if (a.term < 1 || a.term > SLOTS) return [409, 'the console stays open (it holds installs and messages)'];
        const t = S.terminals[a.term];
        if (t) {
          stopTimer(t.sid);
          S.lines.delete(t.sid);
        }
        S.terminals[a.term] = null;
        pushState();
        return [200, ''];
      }
      case 'redetect': {
        S.detecting = true;
        S.status_line = 'detecting installed tools…';
        pushState();
        setTimeout(() => {
          S.detecting = false;
          S.status_line = 'detection done';
          toast('info', `detected ${status.filter((s) => s === 'installed').length} installed tools`);
          pushState();
        }, 900);
        return [200, ''];
      }
      case 'hashid': {
        out(0, `$ hashid ${a.hash.trim()}`, ...identify(a.hash));
        const h = (S.history['\u0000hashid'] ||= []);
        h.push(a.hash.trim());
        pushState();
        return [200, ''];
      }
      case 'challenge': {
        const c = CHALLENGES.find((c) => c.id === a.id);
        if (!c) return [409, `no challenge "${a.id}"`];
        S.challenge = c;
        out(0, `:: challenge: ${c.event} / ${c.name}${c.target ? ` · ${c.target}` : ''} · logs, flags and files go to C:\\Users\\you\\ctf\\${c.event}\\${c.name}`);
        S.status_line = `challenge: ${c.event} / ${c.name}`;
        pushState();
        return [200, ''];
      }
      default:
        return [400, 'unknown action'];
    }
  }

  const realFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : input.url;
    if (/(^|\/)api\/action$/.test(url)) {
      let a;
      try { a = JSON.parse(init?.body || '{}'); } catch { return new Response('bad JSON', { status: 400 }); }
      const [code, text] = handle(a);
      return new Response(text, { status: code, headers: { 'Content-Type': 'text/plain' } });
    }
    const m = /api\/log\?sid=(\d+)/.exec(url);
    if (m) {
      const sid = Number(m[1]);
      const t = S.terminals.find((x) => x && x.sid === sid);
      const body = [`# winsploit run log (demo)`, `# tool: ${t?.title ?? '?'}`, `# started: ${new Date().toISOString()}`, '', ...(S.lines.get(sid) || [])].join('\r\n');
      return new Response(body, { status: 200, headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
    }
    return realFetch(input, init);
  };

  // log links are plain <a href>: serve them as downloads from memory
  document.addEventListener('click', (e) => {
    const a = e.target.closest?.('a.log-link');
    if (!a) return;
    e.preventDefault();
    window.fetch(a.getAttribute('href')).then((r) => r.blob()).then((b) => {
      const u = URL.createObjectURL(b);
      const dl = document.createElement('a');
      dl.href = u;
      dl.download = 'winsploit-demo.log';
      dl.click();
      setTimeout(() => URL.revokeObjectURL(u), 1000);
    });
  }, true);
})();
