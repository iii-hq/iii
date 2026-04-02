import * as Twoslash from 'fumadocs-twoslash/ui'
import { CodeBlock, Pre } from 'fumadocs-ui/components/codeblock'
import { ImageZoom } from 'fumadocs-ui/components/image-zoom'
import { Tab, Tabs } from 'fumadocs-ui/components/tabs'
import defaultMdxComponents from 'fumadocs-ui/mdx'
import type { MDXComponents } from 'mdx/types'

export function getMDXComponents(components?: MDXComponents): MDXComponents {
  return {
    ...defaultMdxComponents,
    img: (props) => <ImageZoom {...(props as any)} />,
    pre: ({ ref: _ref, ...props }) => (
      <CodeBlock {...props}>
        <Pre>{props.children}</Pre>
      </CodeBlock>
    ),
    Tabs,
    Tab,
    ...Twoslash,
    ...components,
  }
}
