---
name: doc_tutorial
description: Writing guide for Tutorial documentation — learning-oriented content that teaches users a skill through hands-on, concrete, step-by-step practice. Use when the goal is for the reader to acquire a new skill by doing, not just reading.
---

# Skill: Writing Tutorials (Learning-Oriented)

**Goal**: Allow the user to acquire a skill by doing.

## content_rules

- **Learning Experience**: Focus entirely on the learning experience of the user.
- **Concrete Steps**: Provide exact, ordered steps.
- **Immediate Results**: Ensure the user sees results immediately after actions.
- **Repetition**: Repeat actions only if necessary for muscle memory; otherwise, reference previous steps.
- **No Alternatives**: Provide exactly one way to do the task. Avoid "or you could do this."
- **Concrete Examples**: Use specific filenames, variable names, and code blocks. Avoid abstract concepts.
- **Resist Abstraction**: The temptation to generalize is strong — resist it. Tutorials proceed strictly from the concrete to the particular. Never introduce edge cases, advanced options, or generalizations mid-tutorial. Those belong in Explanation or Reference docs. Naming the failure mode: if you catch yourself writing "or, more generally..." — stop.
- **Robustness**: Every step must be tested and verifiable before publishing. A tutorial that breaks partway through is worse than no tutorial — it abandons the beginner at the worst moment. Tutorials must be actively maintained as the underlying software changes.

## structure

1. **Introduction**: "In this tutorial, you will learn how to..."
2. **Prerequisites**: Briefly list what is strictly needed.
3. **Steps**: Numbered sequence of actions.
4. **Conclusion**: "You have successfully..."
