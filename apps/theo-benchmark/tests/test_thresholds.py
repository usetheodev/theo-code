"""Tests for SOTA thresholds TOML."""

import sys
from pathlib import Path

try:
    import tomllib
except ImportError:
    import tomli as tomllib  # type: ignore[no-redef]

THRESHOLDS_PATH = Path(__file__).resolve().parent.parent.parent.parent / "docs" / "sota-thresholds.toml"


def _load():
    with open(THRESHOLDS_PATH, "rb") as f:
        return tomllib.load(f)


class TestThresholdsLoad:
    def test_toml_loads(self):
        data = _load()
        assert isinstance(data, dict)

    def test_has_meta(self):
        data = _load()
        assert "meta" in data
        assert "verified_date" in data["meta"]
        assert "schema_version" in data["meta"]


class TestThresholdFields:
    def test_every_threshold_has_type(self):
        data = _load()
        for section, values in data.items():
            if section == "meta":
                continue
            for key, threshold in values.items():
                if not isinstance(threshold, dict):
                    continue
                assert "type" in threshold, f"{section}.{key} missing 'type'"
                assert threshold["type"] in ("dod-gate", "research-benchmark-ref"), (
                    f"{section}.{key} has invalid type: {threshold['type']}"
                )

    def test_every_threshold_has_source(self):
        data = _load()
        for section, values in data.items():
            if section == "meta":
                continue
            for key, threshold in values.items():
                if not isinstance(threshold, dict):
                    continue
                assert "source" in threshold, f"{section}.{key} missing 'source'"
                assert len(threshold["source"]) > 0

    def test_every_threshold_has_confidence(self):
        data = _load()
        for section, values in data.items():
            if section == "meta":
                continue
            for key, threshold in values.items():
                if not isinstance(threshold, dict):
                    continue
                assert "confidence" in threshold, f"{section}.{key} missing 'confidence'"
                conf = threshold["confidence"]
                assert 0.0 <= conf <= 1.0, f"{section}.{key} confidence out of range: {conf}"

    def test_dod_gates_have_floor(self):
        data = _load()
        for section, values in data.items():
            if section == "meta":
                continue
            for key, threshold in values.items():
                if not isinstance(threshold, dict):
                    continue
                if threshold.get("type") == "dod-gate":
                    assert "floor" in threshold, f"{section}.{key} is dod-gate but missing 'floor'"


class TestBelowFloor:
    def test_recall_at_5_flagged(self):
        data = _load()
        assert data["retrieval"]["recall_at_5"].get("status") == "BELOW_FLOOR"

    def test_recall_at_10_flagged(self):
        data = _load()
        assert data["retrieval"]["recall_at_10"].get("status") == "BELOW_FLOOR"


class TestRetrievalFloors:
    """Verify all 6 retrieval floors from meeting D6."""

    def test_mrr_floor(self):
        data = _load()
        assert data["retrieval"]["mrr"]["floor"] == 0.90

    def test_recall_5_floor(self):
        data = _load()
        assert data["retrieval"]["recall_at_5"]["floor"] == 0.92

    def test_recall_10_floor(self):
        data = _load()
        assert data["retrieval"]["recall_at_10"]["floor"] == 0.95

    def test_depcov_floor(self):
        data = _load()
        assert data["retrieval"]["depcov"]["floor"] == 0.96

    def test_ndcg_5_floor(self):
        data = _load()
        assert data["retrieval"]["ndcg_at_5"]["floor"] == 0.85

    def test_per_lang_floor(self):
        data = _load()
        assert data["retrieval"]["per_language_recall_at_5"]["floor"] == 0.85


class TestMinimumThresholdCount:
    def test_at_least_20_thresholds(self):
        data = _load()
        count = 0
        for section, values in data.items():
            if section == "meta":
                continue
            for key, threshold in values.items():
                if isinstance(threshold, dict) and "type" in threshold:
                    count += 1
        assert count >= 20, f"Only {count} thresholds defined, need >= 20"
