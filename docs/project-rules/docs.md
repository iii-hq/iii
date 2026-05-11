# Docs rules

Rules for authoring and structuring the iii docs pages.

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
