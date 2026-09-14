//! `/usage`: a fold over the responses recorded in session files, by model
//! and period, with cost only where the catalog knows the price.

mod common;

use common::{env_lock, test_model, Home};
use e::core::providers::catalog::Api;
use e::core::providers::{ResponseMeta, ResponsePurpose, Usage};
use e::core::session::SessionLog;

fn response(
    model: &e::core::providers::catalog::Model,
    at_ms: u64,
    input: u64,
    output: u64,
) -> ResponseMeta {
    let mut meta = ResponseMeta::new(
        model,
        ResponsePurpose::Compaction,
        Some(Usage {
            input,
            output,
            cache_read: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
        }),
    );
    meta.timestamp = at_ms;
    meta
}

#[test]
fn usage_folds_responses_by_model_within_the_period() {
    let _lock = env_lock();
    let home = Home::new("usage");
    let cwd = home.dir.join("ws");
    std::fs::create_dir_all(&cwd).unwrap();
    let mock = test_model("mock", 0, Api::Completions);
    let mut other = test_model("mock", 0, Api::Completions);
    other.id = "other".into();

    let mut a = SessionLog::create(&cwd, "mock/test").unwrap();
    a.append_response(response(&mock, 1_000, 100, 10)).unwrap();
    a.append_response(response(&mock, 5_000, 200, 20)).unwrap();
    let mut b = SessionLog::create(&cwd, "mock/other").unwrap();
    b.append_response(response(&other, 5_000, 1_000, 1))
        .unwrap();
    b.append_response(response(&other, 9_000, 1, 1)).unwrap();
    let paths = vec![a.path().to_path_buf(), b.path().to_path_buf()];

    let all = e::core::usage::report(&paths, None);
    assert_eq!(all.requests, 4);
    assert_eq!(all.usage.input, 1_301);
    assert_eq!(
        all.rows[0].model, "mock/other",
        "most requests first, ties by name"
    );
    assert!(all.cost_usd.is_none(), "an unlisted model has no price");
    assert!(all.rows.iter().all(|r| r.cost_usd.is_none()));

    // A fork copies responses into a new file: the same id is one request.
    let mut forked = SessionLog::create(&cwd, "mock/other").unwrap();
    let copied = e::core::session::responses_in(b.path()).remove(0);
    forked.append_response(copied).unwrap();
    let with_fork = [paths.clone(), vec![forked.path().to_path_buf()]].concat();
    assert_eq!(e::core::usage::report(&with_fork, None).requests, 4);

    let recent = e::core::usage::report(&paths, Some(5_000));
    assert_eq!(
        recent.requests, 3,
        "responses before the period are excluded"
    );
    assert_eq!(recent.usage.output, 22);

    let table = e::core::usage::markdown(&recent, "last 7 days");
    assert!(table.starts_with("| model | requests |"));
    assert!(table.contains("| mock/test | 1 |"));
    assert!(table.contains("| **total** | 3 |"));
    assert!(table.contains("| — |"), "unknown cost is a dash");
    assert!(e::core::usage::markdown(
        &e::core::usage::report(&paths, Some(u64::MAX)),
        "last 24 hours"
    )
    .contains("No responses recorded"));

    assert_eq!(e::core::usage::period_ms("24h"), Some(Some(86_400_000)));
    assert_eq!(e::core::usage::period_ms(""), Some(Some(7 * 86_400_000)));
    assert_eq!(e::core::usage::period_ms("all"), Some(None));
    assert_eq!(e::core::usage::period_ms("fortnight"), None);
}
