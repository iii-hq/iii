'use client'

import { SeasonLink, type SeasonLinkProps } from '../shared/season-link'
import { IDEAS } from './ideas'

/** The home link in Diwali dress; loaded only while Diwali is showing. */
export default function DiwaliLink(props: SeasonLinkProps) {
  return <SeasonLink ideas={IDEAS} {...props} />
}
