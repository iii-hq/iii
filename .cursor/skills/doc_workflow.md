---
name: doc_workflow
description: Global workflow and style rules that apply to all documentation types. Always load this file alongside the quadrant-specific skill file. Contains tone guidance, component usage, chunked execution protocol, and structural integrity rules.
---

# Global Documentation Workflow & Style

Apply these rules to ALL documentation tasks.

## 1. Tone and Focus

- **Your Knowledge**: Always make sure you have the relevant context, if you're writing technical documentation but do not have it (ex. code base access, libraries, sdks, diagrams) then always ask the user for it first.
- **Educational Focus**: _(Tutorials, How-to Guides, Reference only)_ Maintain a tone that is strictly factual, goal-oriented, and solution-focused. Explanation docs are exempt — they use a discursive, conceptual tone instead (see `doc_explanation.md`).
- **Technical Purity**: _(Tutorials, How-to Guides, Reference only)_ Describe features and implementation details solely. Focus on "how it works" and "how to use it." Explanation docs focus on "why it works this way" and are not bound by this rule.
- **Positive Framing**: Structure instructions to guide the user toward the correct path.
- **Contextual Awareness**: Assume the user is intelligent but lacks the specific context of this feature, design pattern, or other knowledge.

## 2. Component Usage

- **Cross-Referencing**: When referencing concepts explained elsewhere, utilize `<Callout>` components to link to existing documentation rather than restating the information.
- **Fumadocs Standard**: Use the default Fumadocs syntax for callouts. Adapt to other frameworks only if the project structure explicitly demands it.
  - Example: `<Callout type="info">See [Concept Name](./path) for details.</Callout>`
  - Types: `info`, `warn`, `error`, `tip`.

## 3. Execution Protocol (Chunked)

- **Phase 1: Plan**: Outline the file structure and section headers. Pause for confirmation.
- **Phase 2: Execution**: Write content in cohesive chunks (e.g., one complete section or logical step).
- **Phase 3: Review**: Stop after each cohesive chunk.
  - Ask the user: "Review this section. Shall I proceed to [Next Section]?"
  - If the user makes changes relevant to upcoming changes in the plan then take those changes into account.
  - If the user makes changes to the current change then verify their change and prompt them if clarification is needed.
  - If the user makes changes relevant to past changes then outline and suggest making necessary additional revisions to the user.
- **Phase 4: Commit**: Upon completing a logical set of changes (e.g., a full file or a major edit), suggest a Git commit message.
  - Format: `scope: concise description of change`

## 4. Quadrant Integrity (Anti-Contamination)

Each quadrant must stay pure. Documentation naturally drifts — resist it.

- **If you find yourself explaining *why* something works inside a How-to Guide** → stop. Extract it to an Explanation doc and link to it instead.
- **If you find yourself listing all available options inside a Tutorial** → stop. That content belongs in Reference. Link to it.
- **If you find yourself writing step-by-step instructions inside an Explanation doc** → stop. Link to the relevant How-to Guide.
- **If your Reference entry starts coaching the user on what to do** → remove it. Link to a How-to Guide.

Cross-referencing is correct. Embedding foreign content is contamination. When in doubt, link — don't inline.

## 5. Structural Integrity

- **Cohesion**: Ensure every sentence serves the specific goal of the document type (Tutorial vs. Reference).
- **Location**: Always prefer existing file structure over prompting the user to restructure. Avoid any restructuring that would impact SEO. Place files in directories that match their intent (e.g., `/tutorials`, `/how-to`, `/reference`, `/explanation`). Suggest moving files if they are misplaced.
