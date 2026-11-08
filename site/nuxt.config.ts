// Winsploit website: landing page, docs and the live web-view demo.
// Static: `npm run generate` writes .output/public, deployed to GitHub Pages.
// The base path comes from NUXT_APP_BASE_URL (/winsploit/ on Pages).
export default defineNuxtConfig({
  compatibilityDate: '2026-10-01',
  devtools: { enabled: false },
  modules: ['@nuxt/content', '@nuxt/fonts'],
  components: [{ path: '~/components', pathPrefix: false }],
  css: ['~/assets/css/tokens.css', '~/assets/css/base.css', '~/assets/css/prose.css'],

  app: {
    head: {
      htmlAttrs: { lang: 'en' },
      title: 'Winsploit — the native-Windows security toolbox',
      meta: [
        { name: 'viewport', content: 'width=device-width, initial-scale=1' },
        { name: 'theme-color', content: '#050c0d' },
        { name: 'color-scheme', content: 'dark' },
        {
          name: 'description',
          content:
            'Find, install and run curated pentest and CTF tools natively on Windows, from one keyboard-driven terminal dashboard or your browser. No WSL. No VM.',
        },
        { property: 'og:type', content: 'website' },
        { property: 'og:site_name', content: 'Winsploit' },
        { name: 'twitter:card', content: 'summary_large_image' },
      ],
      link: [{ rel: 'icon', type: 'image/svg+xml', href: `${process.env.NUXT_APP_BASE_URL || '/'}favicon.svg` }],
    },
  },

  fonts: {
    families: [
      { name: 'Geist', provider: 'google', weights: [300, 400, 500, 600, 700] },
      { name: 'Geist Mono', provider: 'google', weights: [400, 500, 600] },
      { name: 'Silkscreen', provider: 'google', weights: [400, 700] },
    ],
  },

  content: {
    experimental: { sqliteConnector: 'native' },
    build: {
      markdown: {
        toc: { depth: 3, searchDepth: 3 },
        highlight: {
          theme: 'github-dark',
          langs: ['powershell', 'yaml', 'bash', 'json', 'rust', 'ini'],
        },
      },
    },
  },

  nitro: {
    prerender: {
      crawlLinks: true,
      routes: ['/', '/docs', '/catalog', '/search.json'],
      // public/demo/ is a static folder (the real web UI + mock), not a route;
      // check-links still verifies the links to it resolve.
      // (crawled routes keep the base prefix, hence a pattern, not '/demo')
      ignore: [/\/demo(\/|$)/],
      failOnError: true,
    },
  },
});
