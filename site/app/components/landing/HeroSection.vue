<script setup lang="ts">
const cat = useCatalog();
const demo = withBase('demo/');
const title = ref<HTMLElement>();
const tag = ref<HTMLElement>();
const copied = ref(false);
const CLONE = 'git clone https://github.com/tahabenissa/winsploit';

async function copy() {
  try {
    await navigator.clipboard.writeText(CLONE);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1600);
  } catch {
    /* clipboard blocked: the text is selectable anyway */
  }
}

onMounted(async () => {
  const { $gsap, $SplitText } = useNuxtApp();
  await document.fonts?.ready;
  if (prefersReducedMotion() || !title.value) return;
  const split = $SplitText.create(title.value, { type: 'lines,chars', mask: 'lines', linesClass: 'split-line', charsClass: 'split-char' });
  const tl = $gsap.timeline({ delay: 0.25 });
  tl.from(split.chars, { yPercent: 115, duration: 1.4, stagger: 0.022, ease: 'expo.out' })
    .from('.hero__fade', { opacity: 0, y: 24, duration: 1.2, stagger: 0.08, ease: 'expo.out' }, 0.55)
    .from('.hero__rule', { scaleX: 0, transformOrigin: 'left', duration: 1.4, ease: 'expo.inOut' }, 0.3);
  if (tag.value) scramble(tag.value, tag.value.textContent || '', 1100);
});
</script>

<template>
  <section class="hero">
    <HeroCanvas />
    <div class="hero__shade" />
    <div class="hero__content wrap">
      <p class="eyebrow hero__fade">
        <span ref="tag">Native-Windows security toolbox · v{{ cat.version }} alpha</span>
      </p>
      <h1 ref="title" class="hero__title">
        Pentest tools.<br />
        <span class="hero__accent">Native on Windows.</span>
      </h1>
      <div class="hero__rule" aria-hidden="true" />
      <div class="hero__row">
        <p class="hero__sub hero__fade">
          Find, install and run {{ cat.counts.tools }} curated pentest &amp; CTF tools from one keyboard-driven
          dashboard, or from your browser. No WSL. No VM. No guessing what actually works.
        </p>
        <div class="hero__ctas hero__fade">
          <NuxtLink to="/docs/get-started/quickstart" class="btn btn--primary">Get started <ArrowIcon /></NuxtLink>
          <a :href="demo" class="btn">Try the live demo <ArrowIcon /></a>
        </div>
      </div>
      <button class="hero__clone hero__fade" :class="{ 'is-copied': copied }" type="button" @click="copy">
        <span class="t-ok">&gt;</span>
        <code>{{ CLONE }}</code>
        <span class="hero__copy">{{ copied ? 'copied ✓' : 'copy' }}</span>
      </button>
    </div>

    <div class="hero__meta wrap hero__fade" aria-hidden="true">
      <span>[ Rust · ratatui ]</span>
      <span>[ Scoop · pipx ]</span>
      <span>[ MIT ]</span>
      <span class="hero__scroll">Scroll <i /></span>
    </div>
  </section>
</template>

<style scoped>
.hero { position: relative; min-height: 100svh; display: flex; flex-direction: column; justify-content: flex-end; padding: calc(var(--nav-h) + 64px) 0 32px; overflow: hidden; }
.hero__shade { position: absolute; inset: 0; pointer-events: none; background:
  linear-gradient(to top, var(--dg-900) 4%, transparent 45%),
  radial-gradient(ellipse 70% 60% at 22% 70%, color-mix(in srgb, var(--dg-900) 85%, transparent), transparent 70%); }
.hero__content { position: relative; z-index: 1; display: grid; gap: 28px; pointer-events: none; }
.hero__content > * { pointer-events: auto; }
.hero__title { font-size: clamp(48px, 9.4vw, 152px); font-weight: 500; letter-spacing: -.055em; line-height: .92; max-width: 13ch; }
.hero__accent { color: var(--accent); }
.hero__rule { height: 1px; background: linear-gradient(90deg, var(--accent), var(--line-strong) 30%, transparent); }
.hero__row { display: flex; flex-wrap: wrap; gap: 32px 64px; align-items: flex-end; justify-content: space-between; }
.hero__sub { font-size: clamp(17px, 1.45vw, 21px); color: var(--lg-300); max-width: 560px; line-height: 1.5; }
.hero__ctas { display: flex; flex-wrap: wrap; gap: 12px; }
.hero__clone {
  justify-self: start; display: inline-flex; align-items: center; gap: 12px; max-width: 100%;
  padding: 10px 12px 10px 14px; border: 1px dashed var(--line-strong); border-radius: var(--radius);
  background: color-mix(in srgb, var(--dg-900) 70%, transparent); backdrop-filter: blur(6px);
  font-family: var(--font-mono); font-size: 13px; color: var(--lg-300); text-align: left;
  transition: border-color .4s var(--ease-out);
}
.hero__clone code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.hero__clone:hover { border-color: var(--accent); }
.hero__copy { flex: none; padding: 2px 8px; border-radius: 2px; background: var(--surface-3); font-size: 11px; text-transform: uppercase; letter-spacing: .1em; color: var(--text-2); }
.is-copied .hero__copy { background: var(--accent); color: var(--accent-ink); }
.hero__meta {
  position: relative; z-index: 1; display: flex; gap: 24px; margin-top: 56px; padding-top: 20px; border-top: 1px solid var(--line);
  font-family: var(--font-mono); font-size: 12px; letter-spacing: .1em; text-transform: uppercase; color: var(--text-3);
}
.hero__scroll { margin-left: auto; display: inline-flex; align-items: center; gap: 10px; }
.hero__scroll i { width: 1px; height: 22px; background: var(--line-strong); position: relative; overflow: hidden; }
.hero__scroll i::after { content: ''; position: absolute; inset: 0; background: var(--accent); animation: drip 1.8s var(--ease-io) infinite; }
@keyframes drip { 0% { transform: translateY(-100%); } 60%, 100% { transform: translateY(100%); } }
@media (max-width: 640px) {
  .hero__meta span:not(.hero__scroll) { display: none; }
  .hero__ctas, .hero__ctas .btn { width: 100%; }
  .hero__ctas .btn { justify-content: space-between; }
}
</style>
