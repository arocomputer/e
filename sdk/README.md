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

See [docs/sdk.md](../docs/sdk.md) for the surface and its rules, and
[docs/decisions/0002-rust-sdk-package.md](../docs/decisions/0002-rust-sdk-package.md)
for why this is a package of its own. Unstable until the first release
declares a versioning policy.
