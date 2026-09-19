//! Ask e one question about the current directory and stream the answer.
//!
//! ```sh
//! cargo run -p aro-e-sdk --example ask -- "what does this repository do"
//! E_MODEL=anthropic/claude-opus-5 cargo run -p aro-e-sdk --example ask -- "..."
//! ```
//!
//! Text goes to stdout as it streams; tool activity and the final usage
//! line go to stderr, so the answer alone can be piped onward.

use std::io::Write;

use e_sdk::{Event, Session};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prompt: Vec<String> = std::env::args().skip(1).collect();
    if prompt.is_empty() {
        eprintln!("usage: ask <prompt>");
        std::process::exit(2);
    }
    let mut builder = Session::builder();
    if let Ok(model) = std::env::var("E_MODEL") {
        builder = builder.model(model);
    }
    let mut session = builder.build().await?;
    eprintln!("model {}", session.model());

    let mut turn = session.prompt(prompt.join(" "));
    while let Some(event) = turn.next().await {
        match event {
            Event::Text(delta) => {
                print!("{delta}");
                std::io::stdout().flush()?;
            }
            Event::ToolCall {
                name, arguments, ..
            } => eprintln!("→ {name} {arguments}"),
            Event::ToolEnd {
                outcome, summary, ..
            } => eprintln!("  {outcome:?}: {summary}"),
            Event::Warning(text) | Event::Notice(text) => eprintln!("! {text}"),
            _ => {}
        }
    }
    let reply = turn.finish().await?;
    println!();
    eprintln!(
        "{} in / {} out{}",
        reply.usage.input,
        reply.usage.output,
        reply
            .cost_usd
            .map(|usd| format!(" (~${usd:.4})"))
            .unwrap_or_default()
    );
    session.close().await;
    Ok(())
}
