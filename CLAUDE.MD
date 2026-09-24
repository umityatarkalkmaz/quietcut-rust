# Project Instructions

## Code Generation Rules

### Security
- Prioritize input validation and injection protection in all generated code
- Never hardcode secrets (API keys, tokens, passwords, credentials) — use env vars or config

### Naming & Language
- Name functions action-first (`validate`/`fetch`/`parse` + the object), in whatever casing the language uses
- Keep code minimal and clean — no unnecessary abstraction or boilerplate

### Formatting
- Always wrap code in properly language-tagged blocks/files (no unlabeled snippets)

### Explanations & Correctness
- Keep explanations concise by default — expand only when genuinely necessary
- If existing code, requirements, or my instructions are wrong/incomplete/contradictory, correct clearly and explain why — do not silently comply or soften the correction
- Prefer accuracy and consistency over agreement

### Rust
- `unsafe` block usage is forbidden by default — only allowed when necessity is explicitly justified (e.g. FFI, hardware access, proven performance requirement)

### Dependencies & Tooling
- Install project dependencies locally, scoped to the project (`venv` / `node_modules` / per-project `cargo`) — never install into global/system environment unless explicitly justified as a genuine system-wide requirement
- Never hardcode absolute home directory paths — derive them from `$HOME`
- Verify tool availability with `command -v <tool>` before relying on it

### Git
- Commit messages and pull request titles follow Conventional Commits 1.0.0: `type(scope)!: description`, with the types `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, `revert`
- Choose the type by user impact: only `feat`, `fix`, `perf` and breaking changes (`!` or a `BREAKING CHANGE:` footer) produce a release, so documentation, comment, test and tooling changes never use them
- Pull requests are squash-merged, so the pull request title is the line that lands in the changelog

## Output Language

### Everything outside string literals: English
- Identifiers, comments, docstrings, commit messages and branch names are always English
- Prose documents (README.md, CHANGELOG, docs/) default to English; write them in Turkish only when I explicitly ask

### String literals: language depends on who reads them
- User-facing strings use the target user language: UI text, card text, exported or downloadable output, form validation feedback, any message the end user sees
- The target language applies only when I have stated it (in the prompt or in a `Target user language:` line in this file). If none is stated, use English
- Strings the end user never sees are always English: thrown error messages, logs, test names, assertion messages, developer-facing CLI/debug output
- Code-level string data is not prose and stays English: object keys, enum values, config keys, CSS class names, env var names
- If a thrown error can reach the end user, keep the internal message English and map it to a separate user-facing string (e.g. via an error code). Do not surface the internal message directly
- English code containing target-language user-facing strings is not "mixed language". String content is data: it can change independently and may later move into a translation layer

### Chat
- Chat responses to me are Turkish: explanations, questions, confirmations ("should I do X?"), summaries, plans, error diagnosis
