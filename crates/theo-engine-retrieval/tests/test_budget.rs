/// Tests for budget.rs — Token Budget Allocation

use theo_engine_retrieval::budget::{BudgetAllocation, BudgetConfig};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_default_config_sums_to_100_percent() {
    let config = BudgetConfig::default_16k();
    let total =
        config.repo_map_pct + config.module_cards_pct + config.real_code_pct
        + config.task_history_pct + config.reserve_pct;

    assert!(
        (total - 1.0).abs() < 1e-9,
        "default percentages must sum to 1.0, got {total}"
    );
}

#[test]
fn test_allocation_for_16000_tokens() {
    let config = BudgetConfig::default_16k();
    let alloc = config.allocate(16_000);

    // Expected: repo_map=2400 (15%), module_cards=4000 (25%),
    //           real_code=6400 (40%), task_history=2400 (15%), reserve=800 (5%)
    assert_eq!(alloc.repo_map, 2400, "repo_map should be 2400");
    assert_eq!(alloc.module_cards, 4000, "module_cards should be 4000");
    assert_eq!(alloc.real_code, 6400, "real_code should be 6400");
    assert_eq!(alloc.task_history, 2400, "task_history should be 2400");
    assert_eq!(alloc.reserve, 800, "reserve should be 800");
}

#[test]
fn test_allocation_sums_to_at_most_total_budget() {
    let config = BudgetConfig::default_16k();

    for total in [100, 1000, 4096, 8192, 16000, 32768] {
        let alloc = config.allocate(total);
        let sum = alloc.repo_map + alloc.module_cards + alloc.real_code
            + alloc.task_history + alloc.reserve;

        assert!(
            sum <= total,
            "allocation sum ({sum}) exceeds budget ({total}) for total={total}"
        );
    }
}

#[test]
fn test_allocation_zero_budget() {
    let config = BudgetConfig::default_16k();
    let alloc = config.allocate(0);

    assert_eq!(alloc.repo_map, 0);
    assert_eq!(alloc.module_cards, 0);
    assert_eq!(alloc.real_code, 0);
    assert_eq!(alloc.task_history, 0);
    assert_eq!(alloc.reserve, 0);
}

#[test]
fn test_allocation_small_budget_no_token_leakage() {
    let config = BudgetConfig::default_16k();
    let alloc = config.allocate(7);

    // No bucket should exceed its percentage of the total
    let total = 7usize;
    assert!(alloc.repo_map <= total);
    assert!(alloc.module_cards <= total);
    assert!(alloc.real_code <= total);
    assert!(alloc.task_history <= total);
    assert!(alloc.reserve <= total);

    let sum = alloc.repo_map + alloc.module_cards + alloc.real_code
        + alloc.task_history + alloc.reserve;
    assert!(sum <= total, "sum {sum} > total {total}");
}

#[test]
fn test_custom_config_allocates_correctly() {
    // 50% real_code, 50% reserve, everything else 0
    let config = BudgetConfig {
        repo_map_pct: 0.0,
        module_cards_pct: 0.0,
        real_code_pct: 0.5,
        task_history_pct: 0.0,
        reserve_pct: 0.5,
    };
    let alloc = config.allocate(1000);

    assert_eq!(alloc.repo_map, 0);
    assert_eq!(alloc.module_cards, 0);
    assert_eq!(alloc.real_code, 500);
    assert_eq!(alloc.task_history, 0);
    assert_eq!(alloc.reserve, 500);
}

#[test]
fn test_budget_allocation_is_public_struct() {
    // Verifies that BudgetAllocation fields are accessible
    let alloc = BudgetAllocation {
        repo_map: 1,
        module_cards: 2,
        real_code: 3,
        task_history: 4,
        reserve: 5,
    };
    assert_eq!(alloc.repo_map + alloc.module_cards + alloc.real_code + alloc.task_history + alloc.reserve, 15);
}
