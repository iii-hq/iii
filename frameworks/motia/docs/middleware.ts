import { type NextRequest, NextResponse } from 'next/server'

const redirects: Record<string, string> = {
  '/docs/getting-started/build-your-first-motia-app': '/docs/getting-started/quick-start',
  '/docs/concepts/workbench': '/docs/concepts/overview',
  '/docs/deployment-guide/motia-cloud/features': '/docs/deployment-guide/getting-started',
  '/docs/deployment-guide/motia-cloud/deployment': '/docs/deployment-guide/getting-started',
  '/docs/deployment-guide/motia-cloud/continuous-deployment': '/docs/deployment-guide/getting-started',
}

export function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl

  if (redirects[pathname]) {
    return NextResponse.redirect(new URL(redirects[pathname], request.url), 301)
  }

  if (pathname.endsWith('.mdx')) {
    const cleanPath = pathname.replace('.mdx', '')
    return NextResponse.redirect(new URL(cleanPath, request.url), 301)
  }

  return NextResponse.next()
}

export const config = {
  matcher: ['/((?!_next/static|_next/image|favicon.ico|.*\\.(?:svg|png|jpg|jpeg|gif|webp)$).*)'],
}
