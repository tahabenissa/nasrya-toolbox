<script setup lang="ts">
import { REPO } from '~/composables/catalog';

const hidden = ref(false);
const scrolled = ref(false);
const open = ref(false);
const stars = ref<number | null>(null);
const demo = withBase('demo/');

const links = [
  { to: '/#features', label: 'Features' },
  { to: '/#catalog', label: 'Catalog' },
  { to: '/#web-view', label: 'Web view' },
  { to: '/docs', label: 'Docs' },
];

let last = 0;
function onScroll() {
  const y = window.scrollY;
  scrolled.value = y > 24;
  hidden.value = y > 300 && y > last && !open.value;
  last = y;
}

onMounted(async () => {
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
  try {
    const cached = sessionStorage.getItem('ws-stars');
    if (cached) stars.value = Number(cached);
    else {
      const r = await fetch('https://api.github.com/repos/tahabenissa/winsploit');
      if (r.ok) {
        stars.value = (await r.json()).stargazers_count;
        sessionStorage.setItem('ws-stars', String(stars.value));
      }
    }
  } catch {
    /* offline or rate-limited: the button shows without a count */
  }
});
onBeforeUnmount(() => window.removeEventListener('scroll', onScroll));

const route = useRoute();
watch(() => route.fullPath, () => (open.value = false));
</script>

<template>
  <header class="nav" :class="{ 'is-hidden': hidden, 'is-scrolled': scrolled, 'is-open': open }">
    <div class="nav__inner wrap">
      <NuxtLink to="/" class="nav__brand" aria-label="Winsploit home">
        <LogoMark />
        <span class="pixel">WINSPLOIT</span>
      </NuxtLink>

      <nav class="nav__links" aria-label="Main">
        <NuxtLink v-for="l in links" :key="l.to" :to="l.to" class="nav__link">
          <span class="nav__roll"><span :data-text="l.label">{{ l.label }}</span></span>
        </NuxtLink>
        <a :href="demo" class="nav__link"><span class="nav__roll"><span data-text="Live demo">Live demo</span></span></a>
      </nav>

      <div class="nav__actions">
        <a :href="REPO" class="nav__gh" target="_blank" rel="noopener">
          <GithubIcon />
          <span>GitHub</span>
          <b v-if="stars !== null" class="nav__stars">★ {{ stars }}</b>
        </a>
        <NuxtLink to="/docs/get-started/quickstart" class="btn btn--primary btn--sm nav__cta">
          Get Winsploit
          <ArrowIcon />
        </NuxtLink>
        <button class="nav__burger" :aria-expanded="open" aria-label="Menu" @click="open = !open"><i /><i /></button>
      </div>
    </div>

    <div class="nav__sheet" :aria-hidden="!open">
      <NuxtLink v-for="(l, i) in links" :key="l.to" :to="l.to" :style="{ '--i': i }">{{ l.label }}</NuxtLink>
      <a :href="demo" :style="{ '--i': links.length }">Live demo</a>
      <a :href="REPO" :style="{ '--i': links.length + 1 }">GitHub</a>
    </div>
  </header>
</template>

<style scoped>
.nav {
  position: fixed; inset: 0 0 auto; z-index: 50; height: var(--nav-h);
  transition: transform .6s var(--ease-out), background-color .5s var(--ease-out), border-color .5s var(--ease-out);
  border-bottom: 1px solid transparent;
}
.nav.is-scrolled {
  background: color-mix(in srgb, var(--dg-900) 72%, transparent);
  backdrop-filter: blur(14px) saturate(1.4); -webkit-backdrop-filter: blur(14px) saturate(1.4);
  border-color: var(--line);
}
.nav.is-hidden { transform: translateY(-100%); }
.nav__inner { height: 100%; display: flex; align-items: center; gap: 32px; }
.nav__brand { display: flex; align-items: center; gap: 12px; font-size: 15px; letter-spacing: .06em; }
.nav__links { display: flex; gap: 4px; margin-left: 24px; }
.nav__link {
  padding: 8px 12px; font-family: var(--font-mono); font-size: 13px; text-transform: uppercase;
  letter-spacing: .08em; color: var(--text-2); transition: color .4s var(--ease-out);
}
.nav__link:hover, .nav__link.router-link-exact-active { color: var(--text); }
/* text rolls up to a green copy on hover */
.nav__roll { display: inline-block; line-height: 1.3; height: 1.3em; overflow: hidden; vertical-align: top; }
.nav__roll > span { display: block; position: relative; transition: transform .5s var(--ease-out); }
.nav__roll > span::after { content: attr(data-text); position: absolute; left: 0; top: 100%; color: var(--accent); }
.nav__link:hover .nav__roll > span { transform: translateY(-100%); }
.nav__actions { margin-left: auto; display: flex; align-items: center; gap: 12px; }
.nav__gh {
  display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 12px;
  border: 1px solid var(--line); border-radius: var(--radius);
  font-family: var(--font-mono); font-size: 12px; text-transform: uppercase; letter-spacing: .08em; color: var(--text-2);
  transition: border-color .4s var(--ease-out), color .4s var(--ease-out);
}
.nav__gh:hover { border-color: var(--line-strong); color: var(--text); }
.nav__stars { font-weight: 500; color: var(--accent); }
.nav__burger { display: none; width: 40px; height: 40px; position: relative; }
.nav__burger i { position: absolute; left: 10px; right: 10px; height: 1.5px; background: var(--text); transition: transform .5s var(--ease-out); }
.nav__burger i:first-child { top: 15px; }
.nav__burger i:last-child { top: 23px; }
.is-open .nav__burger i:first-child { transform: translateY(4px) rotate(45deg); }
.is-open .nav__burger i:last-child { transform: translateY(-4px) rotate(-45deg); }
.nav__sheet { display: none; }

@media (max-width: 980px) {
  .nav__links, .nav__gh { display: none; }
  .nav__burger { display: block; }
  .nav__sheet {
    display: flex; flex-direction: column; gap: 4px; position: fixed; inset: var(--nav-h) 0 auto; height: calc(100dvh - var(--nav-h));
    padding: 32px var(--gutter); background: var(--dg-900);
    clip-path: inset(0 0 100% 0); transition: clip-path .7s var(--ease-io); pointer-events: none;
  }
  .nav.is-open { background: var(--dg-900); }
  .is-open .nav__sheet { clip-path: inset(0 0 0 0); pointer-events: auto; }
  .nav__sheet a {
    font-size: 40px; letter-spacing: -.03em; padding: 6px 0; border-bottom: 1px solid var(--line);
    opacity: 0; transform: translateY(20px); transition: opacity .6s var(--ease-out), transform .6s var(--ease-out);
  }
  .is-open .nav__sheet a { opacity: 1; transform: none; transition-delay: calc(.15s + var(--i) * .05s); }
}
@media (max-width: 520px) { .nav__cta { display: none; } }
</style>
