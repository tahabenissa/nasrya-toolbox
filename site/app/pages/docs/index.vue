<script setup lang="ts">
definePageMeta({ layout: 'docs' });
useSeoMeta({ title: 'Docs · Winsploit', description: 'Install Winsploit, learn the dashboard and the web view, and extend the tool catalog.' });
const cat = useCatalog();
const { groups } = await useDocsNav();
const demo = withBase('demo/');

const tabs = [
  {
    label: 'From source',
    lines: [
      ['c', '# 1. Scoop (skip if you have it)'],
      ['', 'Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser'],
      ['', 'Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression'],
      ['', ''],
      ['c', '# 2. Build Winsploit (Rust 1.87+)'],
      ['', 'git clone https://github.com/tahabenissa/winsploit'],
      ['', 'cd winsploit'],
      ['', 'cargo build --release'],
      ['', ''],
      ['c', '# 3. Launch the dashboard'],
      ['k', '.\\target\\release\\winsploit'],
    ],
  },
  {
    label: 'Release zip',
    lines: [
      ['c', '# Download winsploit-*-windows-x64.zip from Releases, then:'],
      ['', 'Expand-Archive .\\winsploit-*-windows-x64.zip -DestinationPath C:\\Tools\\winsploit'],
      ['k', 'C:\\Tools\\winsploit\\winsploit.exe'],
      ['', ''],
      ['c', '# tools.yaml sits next to the exe and is found automatically.'],
      ['c', '# The binary is unsigned: Smart App Control may ask you to allow it.'],
    ],
  },
  {
    label: 'Run',
    lines: [
      ['k', 'winsploit'],
      ['c', '#   the interactive dashboard'],
      ['k', 'winsploit --web'],
      ['c', '#   …and the same session in your browser'],
      ['k', 'winsploit list'],
      ['c', '#   catalog + install status, non-interactive'],
      ['k', 'winsploit hashid 5f4dcc3b5aa765d61d8327deb882cf99'],
      ['c', '#   MD5 · NTLM · MD4, with hashcat -m and john --format'],
    ],
  },
];
const tab = ref(0);
const copied = ref(false);
async function copy() {
  try {
    await navigator.clipboard.writeText(tabs[tab.value]!.lines.filter(([k]) => k !== 'c').map(([, l]) => l).join('\n').trim());
    copied.value = true;
    setTimeout(() => (copied.value = false), 1400);
  } catch { /* clipboard blocked */ }
}

const journeys = {
  Dashboard: [
    { t: 'Get started', l: [['▶', 'Quick start', '/docs/get-started/quickstart'], ['⤓', 'Installation', '/docs/get-started/installation'], ['✦', 'First run & tour', '/docs/get-started/first-run']] },
    { t: 'Use', l: [['⌨', 'Keyboard reference', '/docs/using/keyboard'], ['▸', 'Running tools', '/docs/using/running-tools'], ['▦', 'Multiple terminals', '/docs/using/terminals'], ['⌕', 'Output & find', '/docs/using/output']] },
    { t: 'Extend', l: [['≡', 'tools.yaml reference', '/docs/catalog/tools-yaml'], ['+', 'Adding a tool', '/docs/catalog/adding-a-tool'], ['◐', 'verify & notes', '/docs/catalog/verify-and-notes'], ['⛁', 'Winsploit bucket', '/docs/catalog/bucket']] },
    { t: 'Operate', l: [['⛨', 'Security model', '/docs/security/model'], ['☰', 'Logs', '/docs/using/logs'], ['◑', 'Appearance', '/docs/using/appearance'], ['⚙', 'Paths & env vars', '/docs/reference/paths']] },
  ],
  'Web view': [
    { t: 'Get started', l: [['◍', 'Web view overview', '/docs/web-view/overview'], ['⚑', 'Flags & keys', '/docs/web-view/flags'], ['▶', 'Live demo', 'demo']] },
    { t: 'Use', l: [['▸', 'Run, install, stop', '/docs/web-view/overview#what-you-can-do'], ['▦', 'Grid of terminals', '/docs/web-view/overview#keys-in-the-browser'], ['#', 'Hash ID', '/docs/using/hashid']] },
    { t: 'Share', l: [['⇄', 'LAN mode', '/docs/web-view/security#lan-mode'], ['⛨', 'Security', '/docs/web-view/security']] },
    { t: 'Operate', l: [['☰', 'Download run logs', '/docs/using/logs'], ['⚙', 'CLI reference', '/docs/reference/cli']] },
  ],
} as const;
const jtab = ref<keyof typeof journeys>('Dashboard');
const href = (to: string) => (to === 'demo' ? demo : to);
</script>

<template>
  <div class="dhome">
    <DocsSidebar :groups="groups" />
    <main class="dhome__main">
      <section class="dhero">
        <div class="dhero__copy">
          <p class="eyebrow">Winsploit docs</p>
          <h1>Start running tools<br /><span class="accent">natively on Windows</span></h1>
          <p class="dhero__sub">
            Everything you need to install Winsploit, drive the dashboard and the web view, and extend a catalog of
            <Stat k="tools" /> pentest &amp; CTF tools. From first launch to your own <code>tools.yaml</code> entry.
          </p>
          <div class="dhero__chips">
            <NuxtLink to="/docs/get-started/quickstart" class="chip"><i>▶</i> Quick start</NuxtLink>
            <NuxtLink to="/docs/using/keyboard" class="chip"><i>⌨</i> Keyboard reference</NuxtLink>
            <NuxtLink to="/catalog" class="chip"><i>▦</i> Tool catalog</NuxtLink>
          </div>
        </div>
        <div class="dcode">
          <div class="dcode__tabs">
            <button v-for="(t, i) in tabs" :key="t.label" class="mono" :class="{ 'is-on': tab === i }" @click="tab = i">{{ t.label }}</button>
            <button class="dcode__copy mono" @click="copy">{{ copied ? 'Copied ✓' : 'Copy' }}</button>
          </div>
          <pre class="dcode__pre"><code><span v-for="([k, l], i) in tabs[tab]!.lines" :key="`${tab}-${i}`" class="dcode__line" :class="k === 'c' ? 't-com' : k === 'k' ? 't-ok' : ''">{{ l || ' ' }}
</span></code></pre>
        </div>
      </section>

      <section class="dsec">
        <header class="dsec__head">
          <p class="eyebrow">Interfaces</p>
          <h2>Choose how you work</h2>
          <p>One session, three ways in. The terminal is always in charge.</p>
        </header>
        <div class="plat">
          <article class="plat__card">
            <div class="plat__pic" aria-hidden="true"><span class="pixel">&gt;_</span></div>
            <h3>Dashboard</h3>
            <p>The keyboard-driven terminal UI: browse, install and run, with up to <Stat k="slots" /> tools streaming side by side.</p>
            <div class="plat__links">
              <NuxtLink to="/docs/get-started/quickstart">▶ Quick start</NuxtLink>
              <NuxtLink to="/docs/using/dashboard">▦ Dashboard tour</NuxtLink>
              <NuxtLink to="/docs/using/keyboard">⌨ Keys</NuxtLink>
            </div>
          </article>
          <article class="plat__card">
            <div class="plat__pic" aria-hidden="true"><span class="pixel">◍</span></div>
            <h3>Web view</h3>
            <p>The same live session in a browser tab: every terminal, installs and hash IDs. Off by default; loopback unless you ask for LAN.</p>
            <div class="plat__links">
              <NuxtLink to="/docs/web-view/overview">◍ Overview</NuxtLink>
              <NuxtLink to="/docs/web-view/security">⛨ Security</NuxtLink>
              <a :href="demo">▶ Live demo</a>
            </div>
          </article>
          <article class="plat__card">
            <div class="plat__pic" aria-hidden="true"><span class="pixel">$</span></div>
            <h3>Command line</h3>
            <p>Non-interactive commands for scripts and quick checks: list the catalog with install status, or identify a hash.</p>
            <div class="plat__links">
              <NuxtLink to="/docs/reference/cli">≡ CLI reference</NuxtLink>
              <NuxtLink to="/docs/using/hashid">#  Hash identifier</NuxtLink>
            </div>
          </article>
        </div>
      </section>

      <section class="dsec">
        <header class="dsec__head">
          <p class="eyebrow">Journey</p>
          <h2>From install to report</h2>
          <p>Follow the lifecycle, or jump to what you need.</p>
        </header>
        <div class="jtabs">
          <button v-for="k in Object.keys(journeys)" :key="k" class="mono" :class="{ 'is-on': jtab === k }" @click="jtab = k as any">{{ k }}</button>
        </div>
        <div class="journey">
          <div v-for="(step, i) in journeys[jtab]" :key="jtab + step.t" class="journey__step">
            <p class="journey__n mono">{{ String(i + 1).padStart(2, '0') }}</p>
            <h3>{{ step.t }}</h3>
            <ul>
              <li v-for="[icon, label, to] in step.l" :key="label">
                <a v-if="to === 'demo'" :href="href(to)"><i>{{ icon }}</i>{{ label }}</a>
                <NuxtLink v-else :to="to"><i>{{ icon }}</i>{{ label }}</NuxtLink>
              </li>
            </ul>
          </div>
        </div>
      </section>

      <section class="dsec">
        <header class="dsec__head">
          <p class="eyebrow">Catalog</p>
          <h2>{{ cat.counts.tools }} tools in {{ cat.counts.categories }} categories</h2>
          <p>Straight from <code>tools.yaml</code>. Click a category to browse it.</p>
        </header>
        <div class="cats">
          <NuxtLink v-for="c in cat.categories" :key="c.id" :to="{ path: '/catalog', query: { category: c.id } }" class="cats__item">
            <b>{{ c.label }}</b><span class="mono">{{ c.tools.length }}</span>
            <small>{{ c.blurb }}</small>
          </NuxtLink>
        </div>
      </section>

      <section class="dsec dsec--last">
        <header class="dsec__head">
          <p class="eyebrow">Resources</p>
          <h2>Keep going</h2>
        </header>
        <CardGroup :cols="3">
          <Card title="What's new" icon="★" to="/docs/project/changelog">Everything since v{{ cat.version }}.</Card>
          <Card title="Security model" icon="⛨" to="/docs/security/model">What Winsploit trusts, and what it never does.</Card>
          <Card title="Architecture" icon="⌘" to="/docs/project/architecture">Event loop, sessions, detection, the web hub.</Card>
          <Card title="Contributing" icon="+" to="/docs/project/contributing">Add a tool, run the tests, open a PR.</Card>
          <Card title="Live demo" icon="▶" :to="demo.startsWith('http') ? demo : undefined"><a :href="demo">Try the web view in your browser →</a></Card>
          <Card title="GitHub" icon="⌥" to="https://github.com/tahabenissa/winsploit">Source, issues and releases.</Card>
        </CardGroup>
      </section>
    </main>
  </div>
</template>

<style scoped>
.dhome { display: grid; grid-template-columns: 280px minmax(0, 1fr); max-width: 1600px; margin: 0 auto; }
.dhome__main { min-width: 0; padding: 0 clamp(16px, 4vw, 64px) 96px; }
.dhero { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.05fr); gap: 48px; align-items: center; padding: 64px 0 72px; border-bottom: 1px solid var(--line); }
.dhero__copy { display: grid; gap: 22px; justify-items: start; }
.dhero h1 { font-size: clamp(38px, 4.4vw, 60px); letter-spacing: -.045em; }
.dhero__sub { color: var(--text-2); font-size: 17.5px; line-height: 1.55; max-width: 520px; }
.dhero__sub code, .dsec__head code { font-size: .88em; color: var(--accent); }
.dhero__chips { display: flex; flex-wrap: wrap; gap: 8px; }
.chip { display: inline-flex; align-items: center; gap: 8px; height: 38px; padding: 0 14px; border: 1px solid var(--line-strong); border-radius: 999px; font-size: 14px; color: var(--lg-100); transition: border-color .3s, background-color .3s; }
.chip i { font-style: normal; color: var(--accent); font-size: 12px; }
.chip:hover { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, transparent); }
.dcode { border: 1px solid var(--line-strong); border-radius: 12px; overflow: hidden; background: var(--dg-800); box-shadow: 0 40px 100px -40px rgba(0, 0, 0, .8); min-width: 0; }
.dcode__tabs { display: flex; align-items: center; gap: 2px; padding: 0 8px; border-bottom: 1px solid var(--line); background: var(--dg-700); overflow-x: auto; }
.dcode__tabs button { position: relative; height: 42px; padding: 0 12px; font-size: 12.5px; color: var(--text-3); white-space: nowrap; }
.dcode__tabs button.is-on { color: var(--accent); }
.dcode__tabs button.is-on::after { content: ''; position: absolute; left: 10px; right: 10px; bottom: -1px; height: 2px; background: var(--accent); }
.dcode__tabs .dcode__copy { margin-left: auto; color: var(--text-2); }
.dcode__pre { margin: 0; padding: 18px; min-height: 290px; font-size: 13px; line-height: 1.75; overflow-x: auto; color: var(--lg-100); }
.dcode__line { display: block; white-space: pre; }
.dsec { padding: 72px 0; border-bottom: 1px solid var(--line); }
.dsec--last { border-bottom: 0; }
.dsec__head { display: grid; gap: 10px; margin-bottom: 32px; }
.dsec__head h2 { font-size: clamp(28px, 3vw, 38px); letter-spacing: -.035em; }
.dsec__head > p:last-child { color: var(--text-2); }
.plat { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 14px; }
.plat__card { display: flex; flex-direction: column; gap: 12px; padding: 22px; border: 1px solid var(--line); border-radius: 12px; background: var(--dg-800); transition: border-color .3s; }
.plat__card:hover { border-color: var(--line-strong); }
.plat__pic { height: 120px; display: grid; place-items: center; border-radius: 8px; background: radial-gradient(circle at 50% 60%, color-mix(in srgb, var(--accent) 16%, transparent), transparent 60%), repeating-linear-gradient(0deg, var(--line) 0 1px, transparent 1px 12px), repeating-linear-gradient(90deg, var(--line) 0 1px, transparent 1px 12px); }
.plat__pic span { font-size: 42px; color: var(--accent); text-shadow: 0 0 30px var(--accent-glow); }
.plat__card h3 { font-size: 22px; letter-spacing: -.02em; }
.plat__card p { color: var(--text-2); font-size: 14.5px; }
.plat__links { margin-top: auto; display: flex; flex-wrap: wrap; gap: 6px; padding-top: 6px; }
.plat__links a { font-size: 13px; padding: 5px 10px; border-radius: 6px; background: var(--surface-2); color: var(--lg-100); transition: color .2s, background-color .2s; }
.plat__links a:hover { color: var(--accent); background: var(--surface-3); }
.jtabs { display: inline-flex; padding: 3px; gap: 3px; border: 1px solid var(--line-strong); border-radius: 8px; margin-bottom: 20px; }
.jtabs button { height: 34px; padding: 0 14px; border-radius: 6px; font-size: 12.5px; color: var(--text-2); }
.jtabs button.is-on { background: var(--surface-3); color: var(--accent); }
.journey { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); border: 1px solid var(--line); border-radius: 12px; overflow: hidden; }
.journey__step { padding: 22px 18px; border-left: 1px solid var(--line); animation: jin .5s var(--ease-out) both; }
.journey__step:nth-child(2) { animation-delay: .05s; } .journey__step:nth-child(3) { animation-delay: .1s; } .journey__step:nth-child(4) { animation-delay: .15s; }
@keyframes jin { from { opacity: 0; transform: translateY(8px); } }
.journey__step:first-child { border-left: 0; }
.journey__n { font-size: 11px; color: var(--accent); margin-bottom: 6px; }
.journey__step h3 { font-size: 17px; margin-bottom: 14px; }
.journey__step ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
.journey__step a { display: flex; gap: 10px; align-items: center; padding: 6px 8px; margin: 0 -8px; border-radius: 6px; font-size: 14px; color: var(--text-2); transition: color .2s, background-color .2s; }
.journey__step a:hover { color: var(--text); background: var(--surface-2); }
.journey__step a i { font-style: normal; width: 16px; text-align: center; color: var(--accent); font-size: 12px; }
.cats { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
.cats__item { display: grid; grid-template-columns: 1fr auto; gap: 4px 8px; padding: 14px 16px; border: 1px solid var(--line); border-radius: 10px; transition: border-color .3s, background-color .3s; }
.cats__item:hover { border-color: color-mix(in srgb, var(--accent) 50%, transparent); background: var(--dg-800); }
.cats__item b { font-weight: 500; }
.cats__item span { color: var(--accent); font-size: 12px; }
.cats__item small { grid-column: 1 / -1; color: var(--text-3); font-size: 12.5px; }
@media (max-width: 1280px) { .dhero { grid-template-columns: 1fr; } }
@media (max-width: 1024px) { .dhome { grid-template-columns: 1fr; } .plat { grid-template-columns: 1fr; } .journey { grid-template-columns: 1fr 1fr; } .journey__step:nth-child(3) { border-left: 0; } .journey__step:nth-child(n + 3) { border-top: 1px solid var(--line); } }
@media (max-width: 560px) { .journey { grid-template-columns: 1fr; } .journey__step { border-left: 0; border-top: 1px solid var(--line); } .journey__step:first-child { border-top: 0; } }
</style>
