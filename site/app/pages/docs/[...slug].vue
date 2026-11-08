<script setup lang="ts">
definePageMeta({ layout: 'docs' });
const route = useRoute();
const { data: page } = await useAsyncData(`doc-${route.path}`, () => queryCollection('docs').path(route.path).first());
if (!page.value) throw createError({ statusCode: 404, statusMessage: 'Page not found', fatal: true });

const { pages, groups } = await useDocsNav();
const at = computed(() => pages.value.findIndex((p) => p.path === route.path));
const prev = computed(() => (at.value > 0 ? pages.value[at.value - 1] : null));
const next = computed(() => (at.value >= 0 && at.value < pages.value.length - 1 ? pages.value[at.value + 1] : null));
const editUrl = computed(() => `https://github.com/tahabenissa/winsploit/edit/main/site/content/${page.value?.stem}.md`);

useSeoMeta({
  title: () => `${page.value?.title} · Winsploit docs`,
  description: () => page.value?.description,
  ogTitle: () => `${page.value?.title} · Winsploit docs`,
  ogDescription: () => page.value?.description,
});
</script>

<template>
  <div class="docs">
    <DocsSidebar :groups="groups" />
    <main class="docs__main">
      <article v-if="page" class="doc">
        <p class="doc__group mono">{{ page.group }}</p>
        <h1 class="doc__title">{{ page.title }} <span v-if="page.badge" class="doc__badge mono">{{ page.badge }}</span></h1>
        <p v-if="page.description" class="doc__lead">{{ page.description }}</p>
        <ContentRenderer :value="page" class="prose" />
        <footer class="doc__foot">
          <a :href="editUrl" target="_blank" rel="noopener" class="doc__edit mono">✎ Edit this page on GitHub</a>
          <nav class="doc__pn">
            <NuxtLink v-if="prev" :to="prev.path" class="doc__pn-link"><span class="mono">← Previous</span><b>{{ prev.title }}</b></NuxtLink>
            <span v-else />
            <NuxtLink v-if="next" :to="next.path" class="doc__pn-link doc__pn-link--next"><span class="mono">Next →</span><b>{{ next.title }}</b></NuxtLink>
          </nav>
        </footer>
      </article>
    </main>
    <aside class="docs__toc">
      <DocsToc v-if="page?.body?.toc?.links" :links="page.body.toc.links" />
    </aside>
  </div>
</template>

<style scoped>
.docs { display: grid; grid-template-columns: 280px minmax(0, 1fr) 240px; max-width: 1600px; margin: 0 auto; }
.docs__main { min-width: 0; padding: 48px clamp(16px, 4vw, 64px) 96px; }
.doc { max-width: 780px; margin: 0 auto; }
.doc__group { font-size: 12px; letter-spacing: .12em; text-transform: uppercase; color: var(--accent); margin-bottom: 12px; }
.doc__title { font-size: clamp(34px, 4vw, 46px); letter-spacing: -.035em; line-height: 1.08; }
.doc__badge { font-size: 11px; vertical-align: middle; padding: 3px 8px; border-radius: 4px; background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent); letter-spacing: .08em; text-transform: uppercase; }
.doc__lead { margin-top: 14px; font-size: 19px; line-height: 1.5; color: var(--text-2); }
.doc__foot { margin-top: 72px; padding-top: 24px; border-top: 1px solid var(--line); display: grid; gap: 24px; }
.doc__edit { font-size: 12.5px; color: var(--text-3); }
.doc__edit:hover { color: var(--accent); }
.doc__pn { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
.doc__pn-link { display: grid; gap: 4px; padding: 16px 18px; border: 1px solid var(--line); border-radius: 8px; transition: border-color .3s var(--ease-out), background-color .3s var(--ease-out); }
.doc__pn-link:hover { border-color: color-mix(in srgb, var(--accent) 50%, transparent); background: var(--dg-800); }
.doc__pn-link span { font-size: 11.5px; color: var(--text-3); letter-spacing: .06em; }
.doc__pn-link b { font-weight: 500; color: var(--text); }
.doc__pn-link--next { text-align: right; }
.docs__toc { padding: 48px 20px 0 0; }
@media (max-width: 1280px) { .docs { grid-template-columns: 260px minmax(0, 1fr); } .docs__toc { display: none; } }
@media (max-width: 1024px) { .docs { grid-template-columns: minmax(0, 1fr); } }
@media (max-width: 560px) { .doc__pn { grid-template-columns: 1fr; } }
</style>
