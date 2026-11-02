# Docs rules

Rules for authoring and structuring the iii docs pages.

## Quadrant integrity (anti-contamination)

Each Diataxis quadrant — tutorial, how-to, reference, explanation — describes what the page is *for*. Pages drift toward neighbouring quadrants on every revision; resist it.

- A how-to page that starts explaining *why* something works has drifted into explanation. Extract the reasoning to an explanation page and link to it.
- A tutorial that lists every available option for a tool has drifted into reference. Pull the option set out to a reference page and link.
- An explanation that hands the reader step-by-step instructions has drifted into how-to. Move the steps to a how-to and link.
- A reference entry that coaches the reader on what to do has drifted into how-to. Strip the coaching; link to a how-to instead.

Cross-referencing other quadrants is correct. Embedding their content is contamination. When in doubt, link — don't inline.

The Vale layer flags some of this via `Diataxis.CrossContamination`, but the rule is content-judgement, not pattern-matching: the AI layer carries the load, and reviewers should too.

The per-quadrant authoring guides — what counts as a tutorial / how-to / reference / explanation, what each one's structure looks like, and what's specifically disallowed in each one — live in `iii-doc-authoring/diataxis/doc_<type>.md` plus `iii-doc-authoring/diataxis/doc_workflow.md`. Treat those as canonical when judging whether a doc is in its quadrant or has drifted out.

## Check the current dev branch against `main` before authoring

Before writing or editing docs, remind the user to diff the current dev branch against `main` (e.g.
`git log main..HEAD`, `git diff main...HEAD`) to surface in-flight changes that affect what should
be documented — for example, GUI trigger changes, renamed concepts, or new/removed surfaces landed
on the dev branch but not yet reflected in `main`. Skipping this check is a recurring source of
docs that contradict the latest behavior.

## Migrated content is minimal

When porting content into an iii docs page, write only the section title plus at most one sentence
describing what the section _should_ contain. Do not paste original prose, tables, or code blocks.
The point is to mark the slot, not to author the page.

## `expanding-iii/` docs scope

"Expanding iii" means expanding an iii _system_ with more workers and functionality (deploying /
wiring up / integrating additional workers). It is **not** about adding code to the iii engine
itself.

All iii expansion is worker expansion. Content about _authoring_ a worker (implementing engine
traits, building a custom worker package) does not belong in `expanding-iii/`. See
[`workers.md`](./workers.md) for where worker-authoring content goes (outside the iii docs
entirely).
