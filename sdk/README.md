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
cargo run -p e-sdk --example ask -- "what does this repository do"
```

See [docs/sdk.md](../docs/sdk.md) for the API, package boundary, and usage rules.
The API is unstable until its first release declares a versioning policy.

## Versioning

`e-sdk` is published on crates.io and its versions mean something from the
first release:

```sh
cargo add e-sdk
```

- **The SDK's API is the contract**: the types and methods
  [docs/sdk.md](../docs/sdk.md) documents. Everything else is internal.
- **Semantic versioning.** Before 1.0, a release that changes that API without
  a compatible path moves the minor version and says so in the changelog;
  additive and internal changes move the patch. From 1.0 the usual rules apply.
- **One version, two crates.** The application publishes as `intuitum-e`
  (`e` is taken on crates.io) and the SDK as `e-sdk`, from the same tag, so a
  version identifies a matching pair. The application's library target is what
  the SDK is built on; it is not a stable API in itself
  ([compatibility.md](../docs/compatibility.md)).
