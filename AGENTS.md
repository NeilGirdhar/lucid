# Lucid language specification

These instructions apply to coding agents working in this repository.

This repo contains the specification for Lucid, a Python-like language
design sketch, written as Markdown documents built into a site with
[Zensical](https://zensical.org/). There is no compiler or interpreter
here — the "code" is the specification prose and the worked examples
inside it.

## Project structure

- `README.md`—a short pointer for GitHub's own repo view: the core
  principle and a link into the real hub.
- `docs/index.md`—the real hub: the core principle, a worked example, and
  a nested tree of links to every document under `docs/`, grouped by
  theme. This is also the site's homepage.
- `docs/*.md`—one specification document per topic.
  `zensical.toml`'s `nav` orders them to minimize forward references: a
  document should mostly build on documents already covered above it, not
  ones introduced later. A handful of forward pointers are intentional,
  where two topics genuinely reference each other for comparison (e.g.
  generics/traits, dispatch/control-flow); the reverse direction of
  each such pair is already satisfied. Keep `docs/index.md`'s
  documentation list and `zensical.toml`'s `nav` in sync — they describe
  the same tree.
- `zensical.toml`—site config and navigation tree.
- `.github/workflows/docs.yml`—builds and deploys the site to GitHub
  Pages on every push to `main`.

## Core design pillars

- Two kinds of user-defined type: `trait` (obligations, reusable bodies,
  or both, no state) and `class` (owned state, construction, at most one
  class parent).
- Definition-site variance (`+K`/`-K`/`=K`) and mutability views
  (`T`/`~T`/`!T`) visible in the type spelling.
- Julia-style multiple dispatch for binary operators; no reflected methods,
  no `NotImplemented` negotiation.
- Recoverable errors as ordinary return types, checked exhaustively via
  `match`; the `?` operator to propagate them; `raise` narrowed to broken
  invariants.
- `Arguments`/`Parameters` and the anonymous class shape replace `*args`/
  `**kwargs` and `ParamSpec`.
- Zero-deprecation: a one-year release cadence with LLM-driven upgrade
  instructions, which is the standing justification for choosing the
  cleaner rule over the Python-compatible one throughout the spec.

`docs/principles.md` is the canonical statement of these and the
reasoning behind each.

## Markdown conventions

- Name files kebab-case (`type-specification.md`, not
  `type_specification.md`).
- Heading levels are consistent across every file: `#` title, `##` section,
  `###` subsection, `####` sub-subsection. Zensical generates each page's
  table of contents from these automatically — no directive needed.
- Cross-references always name the target file explicitly:
  `[Link text](file.md)`. Markdown has no duplicate-target warning the way
  RST did, so the same link text can repeat freely, pointing at the same
  or different targets, with no anonymous-reference workaround needed.
- A same-file reference to a heading uses that heading's anchor:
  `[Link text](#heading-slug)`, where the slug is the heading text
  lowercased, punctuation stripped, spaces turned to hyphens.
- After any edit, verify the whole site still builds cleanly:

  ```
  uv run zensical build --clean --strict
  ```

  Strict mode aborts the build on any warning (a broken link, an
  unresolved reference) — a clean run is the bar for "done," not just "no
  exception raised."
- Every design claim in the spec should be grounded: motivate a rule with
  a concrete Python (or other language) failure mode before stating
  Lucid's fix, the way the existing documents do throughout.

## Writing guide

- **Make the best change.** Optimize for correctness, clarity, coherence, and consistency, not for the smallest diff. Rewrite, reorganize, or expand the affected passage when doing so improves the result.

- **Clarity first.** The goal is to be understood, not to sound impressive. Prefer plain words over ornamental ones. If a sentence could mean two things, rewrite it to mean one.

- **Every word must tell.** Cut words that carry no information. Remove padding such as *it should be noted that*, *in order to*, *the fact that*, and *it is important to note*.

- **Avoid circumlocution.** Name a thing directly rather than talking around it. Replace “X is what lets Y” with “X lets Y.”

- **Use the active voice and concrete subjects.** Write “The model predicts the label,” not “The label is predicted by the model.” Use the passive when the actor is unknown or beside the point.

- **Choose the common word.** Use an uncommon word only when it adds precision or nuance that the plain word lacks.

- **Prefer verbs to nominalizations.** Write “The model classifies the image,” not “The model performs classification of the image”; write “the optimizer updates the weights,” not “the optimizer makes an update to the weights.”

- **Use one name per thing.** Once a concept has a term, use that term consistently. Do not introduce synonyms merely to avoid repetition. In technical prose, a new term suggests a new concept.

- **Prefer the concrete and specific.** Write “the loss,” not “the relevant quantity,” and “each layer,” not “the various components.”

- **Cut weak qualifiers.** Delete *very*, *rather*, *quite*, *somewhat*, *fairly*, *essentially*, and *basically* unless they convey necessary information.

- **Do not overstate.** Match the strength of a claim to the strength of the evidence. Use *suggests* rather than *proves* when uncertainty remains. Do not use *clearly* or *obviously* in place of an argument.

- **Say what something is, not only what it is not.** Prefer a direct positive statement. Use negation when the contrast itself matters.

- **Avoid canned prose.** Begin with the substance. Do not add generic introductions, conclusions, transitions, summaries, or throat-clearing phrases.

- **Write for the reader.** Assume a technically capable reader who is unfamiliar with the specific idea or change. Supply the context needed to follow the argument without over-explaining standard concepts.

- **Lead from old information to new.** Begin with established context and end with the new or emphatic point.

- **End on the strong word.** Put the most important new information where the sentence lands. Move subordinate qualifications earlier when doing so improves the emphasis.

- **Keep related words together.** Put modifiers next to what they modify, keep the subject close to its verb, and make every pronoun’s antecedent unmistakable.

- **Attach introductory modifiers to their subjects.** Write “After fitting the model, we evaluate it,” not “After fitting the model, its error is evaluated.”

- **Keep sentences manageable.** Length is not itself a fault. A long sentence is good when its parts proceed in a clear order. Break it when clauses accumulate and the reader must hold too much at once. Short by default; long only when the length buys clarity.

- **Use parallel form for parallel ideas.** Coordinate clauses, headings, bullets, and list items should share a grammatical shape.

- **One paragraph, one step.** Give each paragraph one main job. Start a new paragraph when the argument takes a new step.

- **Close each document with a bridge.** End a document with a short passage connecting it to what builds on it next, rather than stopping inside its last subsection.

- **Name the logical relation.** Use words such as *because*, *but*, *therefore*, *instead*, and *for example* when they clarify how one statement relates to the next.

- **Say it once.** State a fact clearly, then refer back to it. Do not repeat the same point in different words merely to soften or emphasize it.

- **Vary sentence structure, not terminology.** Avoid monotonous chains joined by *and*, *but*, or *so*, while keeping the names of concepts stable.

- **Avoid decorative metaphors.** Use an analogy only when it makes a difficult structure easier to understand. Avoid stock phrases and mixed metaphors.

- **Let punctuation expose the structure.** Use a full stop between complete steps, a colon before an explanation or list, and a semicolon only between closely related independent clauses.

- **Use em dashes sparingly.** Prefer a comma, colon, or full stop unless the sentence needs a genuine break in thought.

- **Close em dashes.** Write `word—word`, never `word — word`.

- **Resist unnecessary notation.** Introduce a symbol only when it saves more work than it creates. Do not name a quantity used only once.

- **Italicize only at definition.** Mark a technical term with italics at the sentence that defines it—an “X is Y” or “X: definition follows” construction—and leave every later use plain. Never italicize for emphasis or contrast; carry that with words (*actually*, *instead*, *only*), per “Name the logical relation,” not typography. Re-italicize a term only where a new chapter or section formally reintroduces it in its own self-contained context, not on ordinary reuse within the same passage.

- **Orient the reader before using symbols.** Prefer “The type `T`” to beginning a sentence with a bare `T`.

- **Write arguments forward.** Present a derivation or explanation in the order the reader can understand it, not the reverse order in which it was discovered. Explain the purpose of a step before or as it appears.

- **Use examples only when they clarify.** Give concrete worked examples throughout rather than stating a mechanism only in the abstract. Keep examples minimal, valid, and consistent with the actual code, API, model, or algorithm.

- **Let precision override convention.** Depart from these rules when following them would make the prose less accurate, less clear, or inconsistent with established technical terminology.
