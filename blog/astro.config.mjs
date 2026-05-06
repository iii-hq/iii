import mdx from '@astrojs/mdx'
import { defineConfig } from 'astro/config'

// Served from CloudFront under /blog. Built artifacts are synced to
// s3://<site-bucket>/blog/ — see infra/terraform/website/cloudfront.tf.
export default defineConfig({
  site: 'https://iii.dev',
  base: '/blog',
  trailingSlash: 'always',
  build: {
    format: 'directory',
  },
  integrations: [mdx()],
})
