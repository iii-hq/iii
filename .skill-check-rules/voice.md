# Voice and tone rules

Rules for the prose voice across iii docs pages.

## Manifesto-aligned voice

iii's docs voice should match the website's hero framing — declarative, confident,
paradigm-shift focused. Avoid promotional or tutorial-style framing. State things directly.

The website (`iii-temp/website/index.html`, `iii-temp/website/manifesto.html`) is the canonical
voice reference. Examples of the target voice:

- "Software engineering is an exercise in assembling categories of services."
- "iii fundamentally eliminates this complexity."
- "worker. trigger. function."
- Analogies to paradigm shifts: Unix (everything-is-a-file), React (everything-is-a-component),
  iii (everything-is-a-worker).

## Recurring theme: compose from the registry

Keep driving home — across landing pages, explanations, tutorials, and how-tos — that robust,
interesting systems get built by **combining existing workers from the registry** rather than
writing everything from scratch. Hint at it whenever a section introduces a new primitive,
pattern, or use case: name a registry worker that already does part of the job, or gesture at the
"assemble categories of services" framing from `voice.md`.

Don't make it a slogan or repeat it verbatim. Vary the phrasing, keep it short, and let the
examples carry the weight.

## What to avoid

- Marketing fluff ("the best", "powerful", "revolutionary"). The voice is confident, not
  aggrandizing.
- Tutorial-style framing ("Welcome! Let's get started!"). Be direct.
- Hedging ("you might want to consider"). State the recommendation.
- Disparaging characterizations of other systems or designs. Anything that frames a competitor or
  prior approach as poorly attached ("bolted to the side", "bolted on", "tacked on", "duct-taped",
  "glued on", "retrofitted") is out. The voice does not need to put other tools down to make iii
  look good. State what iii does and let the comparison stand on its own. Vale catches the obvious
  tokens; this rule is for the same idea phrased differently (e.g., "feels stapled together",
  "looks like an afterthought", "shoehorned in").
- Negation-contrast framing ("it's not X, it's Y"; "not just a queue, but a coordination
  primitive"; "this isn't about speed, it's about reliability"). State Y directly: say what the
  thing *is* without first staging what it isn't. The negation adds rhetorical weight, not
  information. Vale catches the common comma/colon-joined forms; this rule is for the same tic
  phrased differently (e.g., "less a framework, more a philosophy", "forget X, think Y", a negated
  sentence immediately followed by its positive restatement). Exception: a genuine disambiguation a
  reader would otherwise get wrong is fine ("this reads from the working directory, not
  `~/.iii/`") because the negation carries real information.
- Promotional capability lists inside non-explainer material. A how-to, install step, or reference
  entry should not pause to introduce a tool with a feature-list bullet block in the shape of
  "X gives your editor / project / system [adjective] [noun]: completions, hover docs, diagnostics,
  …". That pattern belongs in an explanation or a dedicated overview page. In how-to / reference /
  install contexts, state the install command and the user-facing trigger condition; link to the
  capability list if it exists elsewhere. A page can describe what a tool does only when describing
  what the tool does *is* the page's job (i.e., the page is an explanation or overview). Flag this
  pattern wherever it appears, regardless of which tool or extension is being described.
