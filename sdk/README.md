# e-sdk

e's coding agent as a Rust library: create a session against a working
directory, prompt it, read one ordered event stream, get a reply. The same
core the terminal frontend drives — built-in tools, skills and AGENTS.md
context, compaction, session logs, extensions — without a terminal.

```rust
use e_sdk::{Event, Session};

let mut session = Session::builder().cwd("/path/to/project").build().await?;
let mut turn = session.prompt("What does this repository do?");
while let Some(event) = turn.next().await {
    if let Event::Text(delta) = event {
        print!("{delta}");
    }
}
let reply = turn.finish().await?;
session.close().await;
```

```sh
cargo run -p intuitums-e-sdk --example ask -- "what does this repository do"
```

See [docs/guides/extend/sdk.md](../docs/guides/extend/sdk.md) for the API, package boundary, and usage rules.
The API is unstable until its first release declares a versioning policy.

## Versioning

`intuitums-e-sdk` is published on crates.io and its versions mean something from
the first release:

```sh
cargo add intuitums-e-sdk
```

- **The SDK's API is the contract**: the types and methods
  [docs/guides/extend/sdk.md](../docs/guides/extend/sdk.md) documents. Everything else is internal.
- **Semantic versioning.** Before 1.0, a release that changes that API without
  a compatible path moves the minor version and says so in the changelog;
  additive and internal changes move the patch. From 1.0 the usual rules apply.
- **Its own version, not the application's.** The SDK changes for its own
  reasons, so its version tracks only those. It depends on the application crate
  — `intuitums-e`, the npm naming (`@intuitums/e`) since bare `e` is taken on
  crates.io. It pins the exact application version it was tested against; publish
  that application version before publishing the SDK.
- **The application's library target is not a promise.** What the SDK is built
  on is internal; the contract is what this package documents
  ([compatibility.md](../docs/guides/extend/compatibility.md)).

The SDK pins the application crate version it was tested against. Run `./x check`
from the repository root to check all workspace members and compile the packaged
SDK example as an external consumer. See the [SDK guide](../docs/guides/extend/sdk.md)
for the API and release contract.
