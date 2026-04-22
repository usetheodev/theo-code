import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { IntegrityBadge } from "./IntegrityBadge";
import type { IntegrityReport } from "../types";

function mkIntegrity(confidence: number): IntegrityReport {
  return {
    complete: confidence >= 0.95,
    total_events_expected: 100,
    total_events_received: Math.floor(confidence * 100),
    missing_sequences: [],
    drop_sentinels_found: 0,
    writer_recoveries_found: 0,
    confidence,
    schema_version: 1,
  };
}

describe("IntegrityBadge", () => {
  it("test_integrity_badge_green_when_complete", () => {
    render(<IntegrityBadge integrity={mkIntegrity(1.0)} />);
    expect(screen.getByText("Complete")).toBeTruthy();
  });

  it("test_integrity_badge_yellow_when_partial", () => {
    render(<IntegrityBadge integrity={mkIntegrity(0.8)} />);
    expect(screen.getByText(/Partial/)).toBeTruthy();
  });

  it("test_integrity_badge_red_when_degraded", () => {
    render(<IntegrityBadge integrity={mkIntegrity(0.5)} />);
    expect(screen.getByText(/Degraded/)).toBeTruthy();
  });
});
