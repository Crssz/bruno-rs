# bruno-rs

A native-Rust rewrite of the [Bruno](https://github.com/usebruno/bruno) API client — a fast,
single-binary, offline-first, git-native alternative to Postman/Insomnia. No Electron, no web
stack: a pure-Rust core with a [gpui](https://www.gpui.rs) (Zed's GPU UI framework) desktop GUI —
native editor with real tree-sitter syntax highlighting — and a headless CLI.

Collections are plain-text `.bru` files on disk, **byte-for-byte compatible** with Bruno — so your
existing collections open here and `git diff` stays clean.

## Status

Early but functional. You can open Bruno collections, send requests with variables/auth, assert on
responses, and run collections headless in CI.

| Area | State |
|---|---|
| `.bru` parse/serialize | ✅ byte-stable round-trip over a real-Bruno fixture corpus |
| Collections / environments | ✅ folder tree, `bruno.json`, `environments/*.bru` |
| Variables | ✅ `{{var}}` interpolation, collection/env/request scopes, dynamic `{{$guid}}`/`{{$timestamp}}`/`{{$randomInt}}` |
| Send | ✅ query/path params, headers; JSON/text/xml/form/**GraphQL**/**multipart** (file uploads) bodies (async, rustls) |
| Auth | ✅ none / basic / bearer / api-key / **OAuth2** (`client_credentials`+`password`) / **Digest** / **AWS SigV4** · ⏳ OAuth2 browser grants / ntlm / wsse |
| Assertions | ✅ `res.status` / `res.body.*` / `res.headers.*` with `eq`/`neq`/`gt`/`contains`/… |
| Post-response vars | ✅ capture `res.body.*` into variables for request chaining |
| Scripting | ✅ pre/post/test JS in a QuickJS Safe-Mode sandbox — `bru.*` / `req` / `res` / `test` / `expect` + a `pm.*` Postman shim; time/memory/stack-limited |
| Collection runner | ✅ `bru run <dir>`; **data-driven** via `--data <json\|csv>` (one iteration per row, row fields as vars) |
| GUI | ✅ multi-tab editor (**tree-sitter highlighted, fully editable** URL + every `.bru` block) + Save + send + response (status/timing/assertions/tests/console/body, JSON-highlighted) + environments + vault + cookies + devtools + collection runner + Home/recent + sidebar search |
| Import | ✅ Postman v2.1 + cURL (in-app) · ⏳ OpenAPI / Insomnia |
| CLI | ✅ `bru run <file-or-dir> [--env] [--insecure] [--data] [--iterations]` with pass/fail exit codes |
| GUI structured form editors, NTLM/WSSE auth | ⏳ planned |

## Build & run

Requires Rust 1.95+.

```sh
# Desktop app — open a collection folder (or use the in-app picker)
cargo run -p bru-app -- path/to/collection

# Headless — run a request or a whole collection
cargo run -p bru-cli -- run path/to/collection --env staging
cargo run -p bru-cli -- run path/to/request.bru
```

`bru run` exits non-zero if any request errors or any assertion fails, so it drops straight into CI.

## Architecture

A Cargo workspace; all logic lives in UI-agnostic library crates, with the GUI and CLI as thin
shells over the same engine.

```
crates/
  bru-core    domain model + semantic request view, {{interp}}, assertions
  bru-lang    .bru <-> model codec (lossless), collection + env loaders
  bru-http    request execution (reqwest + rustls), timing
  bru-engine  orchestrator: vars -> interpolate -> send -> assert -> capture
  bru-script  QuickJS Safe-Mode sandbox + bru/req/res/test/expect/pm prelude
  bru-cli     `bru` — headless runner
  bru-app     `bruno-rs` — gpui (Zed's GPU UI) desktop app: multi-tab editor w/ tree-sitter highlighting
  bru-import  Postman / OpenAPI / Insomnia / cURL import (planned standalone crate; basic import lives in-app)
```

The `.bru` codec is built on a **lossless block model**: it preserves a file's exact parse order and
surface form and replays it, so round-trips are byte-stable even where Bruno's own serializer would
rewrite a file. The typed request view (method/url/headers/auth/body) is a read-only projection over
that model.

## Tests

```sh
cargo test --workspace
```

Covers byte-stable round-trip over real Bruno fixtures, the semantic layer (interpolation,
assertions, projection), and end-to-end send/assert against hermetic local mock servers (no network
required).

## License

[MIT](LICENSE).

Not affiliated with the Bruno project. `.bru` format compatibility is derived from Bruno's
open-source grammar.
