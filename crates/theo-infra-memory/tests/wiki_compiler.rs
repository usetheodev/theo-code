//! Integration test for the memory-wiki compiler (RM5b).
//!
//! Exercises the compiler through `MockCompilerLLM` from
//! `theo-test-memory-fixtures` so the adapter surface is actually
//! used by a test, not just a unit fixture.

use std::collections::HashSet;

use theo_infra_memory::wiki::{
    CompileBudget, CompilerClient, CompilerResponse, SourceDoc, compile,
};
use theo_test_memory_fixtures::{CompilerResponse as MockResp, MockCompilerLLM};

/// Adapter: the plan's decision was explicit — the fixture lives in
/// `theo-test-memory-fixtures`; the compiler takes an in-crate trait
/// `CompilerClient`. We bridge the two with a thin adapter that lives
/// only in the test file (production never pays for it).
struct FixtureAdapter<'a>(&'a MockCompilerLLM);

impl CompilerClient for FixtureAdapter<'_> {
    fn respond(&self, prompt: &str, temperature: f32, seed: u64) -> CompilerResponse {
        let r = self.0.respond(prompt, temperature, seed);
        CompilerResponse {
            body: r.body,
            token_count: r.token_count,
            cost_usd: r.cost_usd,
        }
    }
}

#[test]
fn mock_llm_drives_compile_to_byte_identical_output() {
    let mock_a = MockCompilerLLM::with_default(MockResp {
        body: "PINNED".into(),
        token_count: 1,
        cost_usd: 0.01,
    });
    let mock_b = MockCompilerLLM::with_default(MockResp {
        body: "PINNED".into(),
        token_count: 1,
        cost_usd: 0.01,
    });

    let sources = vec![
        SourceDoc {
            id: "alpha".into(),
            body: "a".into(),
        },
        SourceDoc {
            id: "beta".into(),
            body: "b".into(),
        },
    ];

    let budget = CompileBudget::default();
    let a = compile(
        &FixtureAdapter(&mock_a),
        sources.clone(),
        &HashSet::new(),
        &budget,
        "page",
    )
    .unwrap();
    let b = compile(
        &FixtureAdapter(&mock_b),
        sources,
        &HashSet::new(),
        &budget,
        "page",
    )
    .unwrap();

    assert_eq!(a.pages[0].body, b.pages[0].body);
}

#[test]
fn mock_llm_records_every_compile_prompt() {
    let mock = MockCompilerLLM::with_default(MockResp {
        body: "R".into(),
        token_count: 0,
        cost_usd: 0.0,
    });
    let sources = vec![
        SourceDoc {
            id: "s1".into(),
            body: "x".into(),
        },
        SourceDoc {
            id: "s2".into(),
            body: "y".into(),
        },
    ];
    compile(
        &FixtureAdapter(&mock),
        sources,
        &HashSet::new(),
        &CompileBudget::default(),
        "p",
    )
    .unwrap();

    let calls = mock.calls();
    assert_eq!(calls.len(), 3, "2 extract + 1 generate");
    assert!(calls[0].prompt.starts_with("extract:s1"));
    assert!(calls[1].prompt.starts_with("extract:s2"));
    assert!(calls[2].prompt.starts_with("generate:p"));
    for c in &calls {
        assert_eq!(c.temperature, 0.0, "temp must be 0 for determinism");
    }
}
