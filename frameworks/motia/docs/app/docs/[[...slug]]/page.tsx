import { Callout } from 'fumadocs-ui/components/callout'
import { Card, Cards } from 'fumadocs-ui/components/card'
import { CodeBlock, Pre } from 'fumadocs-ui/components/codeblock'
import { File, Files, Folder } from 'fumadocs-ui/components/files'
import { ImageZoom, type ImageZoomProps } from 'fumadocs-ui/components/image-zoom'
import { Step, Steps } from 'fumadocs-ui/components/steps'
import { Tab, Tabs } from 'fumadocs-ui/components/tabs'
import { TypeTable } from 'fumadocs-ui/components/type-table'
import { createRelativeLink } from 'fumadocs-ui/mdx'
import { DocsBody, DocsDescription, DocsPage, DocsTitle } from 'fumadocs-ui/page'
import Link from 'next/link'
import { notFound } from 'next/navigation'
import { baseSource, isLegacyExamplesSlug, source } from '@/lib/source'
import { getMDXComponents } from '@/mdx-components'

const isProduction = process.env.NODE_ENV === 'production'

export default async function Page(props: { params: Promise<{ slug?: string[] }> }) {
  const params = await props.params
  if (isProduction && isLegacyExamplesSlug(params.slug)) notFound()

  const page = source.getPage(params.slug)
  if (!page) notFound()

  const MDXContent = page.data.body

  return (
    <DocsPage
      toc={page.data.toc}
      full={page.data.full}
      tableOfContent={{
        style: 'clerk',
      }}
    >
      <DocsTitle>{page.data.title}</DocsTitle>
      <DocsDescription className="mb-2">{page.data.description}</DocsDescription>
      <div className="mb-4 rounded-lg border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950/40 dark:text-amber-100">
        <strong>Motia v1.0 migration:</strong> Upgrading from 0.17? Follow the{' '}
        <Link className="underline underline-offset-2" href="/docs/getting-started/migration-guide">
          0.17 to 1.0 migration guide
        </Link>{' '}
        and{' '}
        <Link className="underline underline-offset-2" href="/docs/getting-started/handler-migration-guide">
          handler migration guide
        </Link>
        .
      </div>
      <DocsBody>
        <MDXContent
          components={getMDXComponents({
            pre: ({ ref: _ref, ...props }) => (
              <CodeBlock {...props}>
                <Pre>{props.children}</Pre>
              </CodeBlock>
            ),
            Card,
            Cards,
            Callout,
            File,
            Folder,
            Files,
            Tab,
            Tabs,
            Step,
            Steps,
            TypeTable,
            img: (props) => <ImageZoom {...(props as ImageZoomProps)} />,
            a: createRelativeLink(baseSource, page),
          })}
        />
      </DocsBody>
    </DocsPage>
  )
}

export async function generateStaticParams() {
  const params = source.generateParams()
  return params
}

export async function generateMetadata(props: { params: Promise<{ slug?: string[] }> }) {
  const params = await props.params
  if (isProduction && isLegacyExamplesSlug(params.slug)) notFound()

  const page = source.getPage(params.slug)
  if (!page) notFound()

  return {
    title: page.data.title,
    description: page.data.description,
  }
}
