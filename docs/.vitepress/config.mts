import { defineConfig } from 'vitepress'

const repository = 'https://github.com/usahp/usahp-core'

export default defineConfig({
  title: 'USAHP',
  description: 'A local, cross-platform switch-event broker for applications.',
  lang: 'en-IE',
  base: '/usahp-core/',
  cleanUrls: true,
  lastUpdated: true,
  sitemap: {
    hostname: 'https://usahp.github.io/usahp-core/'
  },
  head: [
    ['meta', { name: 'theme-color', content: '#16845b' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'USAHP documentation' }],
    ['meta', { property: 'og:title', content: 'USAHP documentation' }],
    ['meta', { property: 'og:description', content: 'Build local apps that respond to normalized switch events.' }]
  ],
  transformHead({ pageData }) {
    const path = pageData.relativePath === 'index.md'
      ? ''
      : pageData.relativePath.replace(/(?:index)?\.md$/, '')
    return [
      ['link', { rel: 'canonical', href: `https://usahp.github.io/usahp-core/${path}` }]
    ]
  },
  themeConfig: {
    siteTitle: 'USAHP',
    search: {
      provider: 'local'
    },
    nav: [
      { text: 'Guide', link: '/quick-start' },
      { text: 'Draft specification', link: '/spec' },
      { text: 'Protocol 0.2', link: '/protocol-v0' },
      { text: 'GitHub', link: repository }
    ],
    sidebar: [
      {
        text: 'Start here',
        items: [
          { text: 'Overview', link: '/' },
          { text: 'How it fits together', link: '/ecosystem' },
          { text: 'Architecture', link: '/architecture' },
          { text: 'Demo app', link: '/demo' },
          { text: 'Roadmap', link: '/roadmap' },
          { text: 'Quick start', link: '/quick-start' }
        ]
      },
      {
        text: 'Use USAHP',
        items: [
          { text: 'Configuration', link: '/configuration' },
          { text: 'USAHP Control utility', link: '/control' },
          { text: 'Client integration', link: '/clients' },
          { text: 'Platform requirements', link: '/platforms' },
          { text: 'Simulator and testing', link: '/simulator' }
        ]
      },
      {
        text: 'Future specification',
        items: [
          { text: 'RFC (Draft)', link: '/spec' },
          { text: 'Draft in plain English', link: '/plain-english' }
        ]
      },
      {
        text: 'Reference',
        items: [
          { text: 'Protocol 0.2', link: '/protocol-v0' },
          { text: 'Limits and exclusions', link: '/limitations' },
          { text: 'Development', link: '/development' }
        ]
      }
    ],
    socialLinks: [
      { icon: 'github', link: repository }
    ],
    editLink: {
      pattern: `${repository}/edit/main/docs/:path`,
      text: 'Edit this page on GitHub'
    },
    lastUpdated: {
      text: 'Updated'
    },
    docFooter: {
      prev: 'Previous',
      next: 'Next'
    },
    footer: {
      message: 'Released under the MIT License.',
      copyright: 'USAHP is an experimental protocol 0.2 project.'
    },
    outline: {
      level: [2, 3],
      label: 'On this page'
    }
  }
})
