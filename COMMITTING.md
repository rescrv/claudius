# Commit Message Best Practices

This document outlines the standards and best practices for writing commit messages in the Claudius
project.

## Overview - Why Good Commits Matter

Clear, well-structured commit messages serve as living documentation for your project, enabling
efficient code review, simplified debugging through tools like `git blame` and `git bisect`, and
facilitating automated tooling for changelog generation and release management. Every commit message
is communication with future developers—including yourself six months from now—who need to
understand not just what changed, but why it changed. Investing time in thoughtful commit messages
pays dividends throughout the project's lifecycle by making the codebase more maintainable and the
development process more collaborative.

## Format Standards - Conventional Commits Structure

All commit messages should follow the conventional commits format: `type(scope): description`, where
type indicates the nature of the change, scope (optional) specifies the affected component, and
description provides a concise summary in 50 characters or less. The subject line should be followed
by a blank line and then an optional body that explains the motivation and implementation approach,
wrapped at 72 characters. Footer elements like breaking change notifications, issue references, and
co-author attributions come after another blank line. This standardized structure enables automated
tooling and makes the git history scannable and meaningful.

## Message Types - Categories with Examples

Use these conventional commit types to categorize your changes: `feat:` for new features, `fix:` for
bug repairs, `docs:` for documentation updates, `style:` for formatting changes that don't affect
code meaning, `refactor:` for code restructuring without functional changes, `test:` for adding or
updating tests, `chore:` for maintenance tasks, `perf:` for performance improvements, `ci:` for
continuous integration changes, and `build:` for build system modifications. For example, `feat:
implement Copy trait for eligible types` clearly indicates a new capability, while `fix: resolve
clippy warnings for needless borrows` signals a bug fix. These types enable automated changelog
generation and help team members quickly understand the nature of each change.

## Writing Guidelines - Language and Style

Write commit subjects in the imperative mood as if completing the sentence "If applied, this commit
will..." Use "Add feature" instead of "Added feature" or "Adding feature." Capitalize the first
letter of the description but don't end with a period. Keep the subject line under 50 characters to
ensure it displays properly in git tools and GitHub interfaces. Be specific and descriptive rather
than vague—"implement FromStr for Model and KnownModel types" is much more helpful than "update
model code." Present tense language keeps commit messages consistent and reads naturally when viewed
in git logs or when cherry-picking commits.

## Body Content - When and How to Explain

Include a commit body when the subject line alone cannot adequately explain the change's motivation,
implementation approach, or impact. Focus on explaining why the change was made rather than simply
restating what the code does—the diff already shows what changed. Use bullet points to organize
multiple related changes, and explain any business logic, architectural decisions, or trade-offs
that informed your implementation. For complex changes, describe the problem being solved, the
approach taken, and any alternative solutions considered. The body should provide enough context for
a reviewer or future maintainer to understand the reasoning behind your decisions.

## Footer Elements - Metadata and References

Use footers to include important metadata that doesn't belong in the main commit message. Reference
related issues with "Closes #123" or "Fixes #456" to automatically link and close issues when the
commit is merged. Include "BREAKING CHANGE:" followed by a description when introducing
backwards-incompatible changes. Add co-author attributions using "Co-authored-by: Name
<email@example.com>" when collaborating with others. For this project, include the standard Claude
Code attribution footer to maintain consistency across AI-assisted commits. Keep footer elements on
separate lines for clarity and tool compatibility.

## Repository-Specific - Claudius Project Standards

This repository follows a specific attribution format for AI-assisted commits that should be
included in the footer of every commit. Use the exact format: "🤖 Generated with [Claude
Code](https://claude.ai/code)" followed by "Co-Authored-By: Claude <noreply@anthropic.com>" on the
next line. This maintains transparency about AI assistance while providing consistent attribution
across the project. When multiple people collaborate on a commit, list all co-authors in the footer.
These attributions help track the development process and acknowledge all contributors, both human
and AI.

## Examples - Good vs Bad Commits

**Good Example:**
```
feat: implement Copy trait for eligible types in src/types/

Add Copy trait to types that only contain Copy-able fields:
- KnownModel: enum with unit variants only
- MessageRole: enum with unit variants only  
- ModelType: enum with single unit variant
- MessageStopEvent: empty struct
- ContentBlockStopEvent: struct with usize field

Also update test code to pass Copy types by value instead of reference,
eliminating clippy::needless_borrows_for_generic_args warnings.

🤖 Generated with [Claude Code](https://claude.ai/code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

**Bad Example:**
```
fix stuff

updated some files
```

The good example clearly states what was done, why it matters, and includes proper attribution,
while the bad example provides no useful information about the changes or their purpose.

## Common Mistakes - What to Avoid

Avoid vague commit messages like "fix bug," "update code," or "WIP" that provide no meaningful
information about the changes. Don't mix unrelated changes in a single commit—each commit should
represent one logical change or feature. Resist the temptation to cram too much detail into the
subject line; use the body for comprehensive explanations. Don't use past tense ("Fixed bug") or
present continuous ("Fixing bug") when imperative mood ("Fix bug") is the standard. Avoid
inconsistent formatting, missing attribution footers, or referencing issues that don't exist.
Remember that commit messages are permanent project documentation—write them with the same care
you'd use for any other important technical document.
