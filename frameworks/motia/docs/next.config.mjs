import { createMDX } from 'fumadocs-mdx/next'

const withMDX = createMDX()

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: false,
  turbopack: true,
  serverExternalPackages: ['typescript', 'twoslash'],
}

export default withMDX(config)
