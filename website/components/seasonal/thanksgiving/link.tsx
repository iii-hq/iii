'use client'

import { SeasonLink, type SeasonLinkProps } from '../shared/season-link'
import { IDEAS } from './ideas'

/** The home link in Thanksgiving dress; loaded only while Thanksgiving is showing. */
export default function ThanksgivingLink(props: SeasonLinkProps) {
  return <SeasonLink ideas={IDEAS} {...props} />
}
