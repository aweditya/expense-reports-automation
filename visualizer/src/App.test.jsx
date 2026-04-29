import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SchemaVisualizer, {
  SECTIONS,
  FIELDS,
  RAW_EDGES,
  EDGES,
  SECTION_BY_ID,
  FIELD_TO_TOP_SECTION,
} from "./App";

// ============================================================================
// Data integrity tests — verify the schema graph is internally consistent
// ============================================================================

describe("Schema data integrity", () => {
  const fieldIds = Object.keys(FIELDS);
  const sectionIds = SECTIONS.map((s) => s.id);

  it("every field references a valid parent section", () => {
    for (const [id, field] of Object.entries(FIELDS)) {
      expect(sectionIds).toContain(field.parent);
    }
  });

  it("every section with a parent references a valid parent section or virtual group", () => {
    // "transaction_lines" is a virtual structural group, not a section with fields.
    const validParents = new Set([...sectionIds, "transaction_lines"]);
    for (const section of SECTIONS) {
      if (section.parent) {
        expect(validParents).toContain(section.parent);
      }
    }
  });

  it("section IDs are unique", () => {
    expect(new Set(sectionIds).size).toBe(sectionIds.length);
  });

  it("section rows are unique", () => {
    const rows = SECTIONS.map((s) => s.row);
    expect(new Set(rows).size).toBe(rows.length);
  });

  it("every edge 'from' references a valid field", () => {
    for (const edge of RAW_EDGES) {
      expect(fieldIds).toContain(edge.from);
    }
  });

  it("every edge 'to' references a valid field", () => {
    for (const edge of RAW_EDGES) {
      expect(fieldIds).toContain(edge.to);
    }
  });

  it("every edge has a valid type", () => {
    const validTypes = ["derive", "required-when", "flag"];
    for (const edge of RAW_EDGES) {
      expect(validTypes).toContain(edge.type);
    }
  });

  it("self-loop edges are explicitly marked", () => {
    for (const edge of RAW_EDGES) {
      if (edge.from === edge.to) {
        expect(edge.selfLoop).toBe(true);
      }
    }
  });

  it("edges marked selfLoop actually have from === to", () => {
    for (const edge of RAW_EDGES) {
      if (edge.selfLoop) {
        expect(edge.from).toBe(edge.to);
      }
    }
  });

  it("every field has a valid source tier", () => {
    const validTiers = ["T1", "T2", "T3"];
    for (const [id, field] of Object.entries(FIELDS)) {
      expect(validTiers).toContain(field.source);
    }
  });

  it("every field has a non-empty label", () => {
    for (const [id, field] of Object.entries(FIELDS)) {
      expect(field.label).toBeTruthy();
      expect(field.label.length).toBeGreaterThan(0);
    }
  });

  it("every field has a non-empty description", () => {
    for (const [id, field] of Object.entries(FIELDS)) {
      expect(field.description).toBeTruthy();
    }
  });

  it("no duplicate edges", () => {
    const edgeKeys = RAW_EDGES.map(
      (e) => `${e.from}→${e.to}:${e.type}`
    );
    expect(new Set(edgeKeys).size).toBe(edgeKeys.length);
  });

  it("requiredWhen fields reference valid field IDs in their predicate prefix", () => {
    // Predicates look like "gi.category == expenses_foreign"
    // The prefix before " == " should be a valid field ID
    for (const [id, field] of Object.entries(FIELDS)) {
      if (field.requiredWhen) {
        const refId = field.requiredWhen.split(" == ")[0].trim();
        expect(fieldIds).toContain(refId);
      }
    }
  });
});

// ============================================================================
// Computed properties tests
// ============================================================================

describe("FIELD_TO_TOP_SECTION", () => {
  it("returns top-level section ID for fields in top-level sections", () => {
    expect(FIELD_TO_TOP_SECTION("gi.category")).toBe("general_information");
    expect(FIELD_TO_TOP_SECTION("pd.start_date")).toBe("per_diem");
  });

  it("returns virtual parent for fields in nested sections", () => {
    // tl_airfare has parent "transaction_lines"
    expect(FIELD_TO_TOP_SECTION("tla.date")).toBe("transaction_lines");
    expect(FIELD_TO_TOP_SECTION("tll.check_in_date")).toBe("transaction_lines");
    expect(FIELD_TO_TOP_SECTION("tlc.meals_included")).toBe("transaction_lines");
  });

  it("returns null for unknown field IDs", () => {
    expect(FIELD_TO_TOP_SECTION("nonexistent.field")).toBeNull();
  });
});

describe("Cross-branch edge computation", () => {
  it("edges within same top-level section are not cross-branch", () => {
    // tla.original_amount → tla.line_amount_usd: both in transaction_lines
    const edge = EDGES.find(
      (e) => e.from === "tla.original_amount" && e.to === "tla.line_amount_usd"
    );
    expect(edge).toBeDefined();
    expect(edge.crossBranch).toBe(false);
  });

  it("edges between different top-level sections are cross-branch", () => {
    // gi.category → tla.original_currency: general_information → transaction_lines
    const edge = EDGES.find(
      (e) => e.from === "gi.category" && e.to === "tla.original_currency"
    );
    expect(edge).toBeDefined();
    expect(edge.crossBranch).toBe(true);
  });

  it("conference meals → per diem deductions is cross-branch", () => {
    const edge = EDGES.find(
      (e) => e.from === "tlc.meals_included" && e.to === "pd.meal_deductions"
    );
    expect(edge).toBeDefined();
    expect(edge.crossBranch).toBe(true);
  });

  it("lodging dates → per diem dates are cross-branch", () => {
    const edge = EDGES.find(
      (e) => e.from === "tll.check_in_date" && e.to === "pd.start_date"
    );
    expect(edge).toBeDefined();
    expect(edge.crossBranch).toBe(true);
  });

  it("self-loop edges are not cross-branch", () => {
    const selfEdges = EDGES.filter((e) => e.selfLoop);
    for (const edge of selfEdges) {
      expect(edge.crossBranch).toBe(false);
    }
  });

  it("edges between different transaction line types are NOT cross-branch (same top-level)", () => {
    // Both tl_airfare and tl_conference are under transaction_lines
    // If such an edge existed, it should NOT be cross-branch.
    // Currently no such edges exist, but verify the logic holds:
    expect(FIELD_TO_TOP_SECTION("tla.date")).toBe(
      FIELD_TO_TOP_SECTION("tlc.conference_start_date")
    );
  });
});

// ============================================================================
// DAG structure tests
// ============================================================================

describe("Dependency DAG structure", () => {
  it("has no cycles in derive edges (excluding self-loops)", () => {
    const deriveEdges = EDGES.filter(
      (e) => e.type === "derive" && !e.selfLoop
    );
    // Build adjacency list and run DFS cycle detection
    const adj = {};
    for (const e of deriveEdges) {
      if (!adj[e.from]) adj[e.from] = [];
      adj[e.from].push(e.to);
    }
    const visited = new Set();
    const inStack = new Set();
    let hasCycle = false;

    function dfs(node) {
      if (inStack.has(node)) {
        hasCycle = true;
        return;
      }
      if (visited.has(node)) return;
      visited.add(node);
      inStack.add(node);
      for (const next of adj[node] || []) {
        dfs(next);
        if (hasCycle) return;
      }
      inStack.delete(node);
    }

    for (const node of Object.keys(adj)) {
      dfs(node);
      if (hasCycle) break;
    }
    expect(hasCycle).toBe(false);
  });

  it("every required-when edge has a predicate", () => {
    const rwEdges = EDGES.filter((e) => e.type === "required-when");
    for (const edge of rwEdges) {
      expect(edge.predicate).toBeTruthy();
    }
  });

  it("cross-branch edge count matches expected", () => {
    const crossBranch = EDGES.filter((e) => e.crossBranch);
    // 3 required-when from gi.category → transaction_lines +
    // 1 gi.category → ts.transaction_type +
    // 1 gi.payee_name → gi.business_purpose_why (same section, not cross-branch) +
    // 1 airfare dest → business purpose where +
    // 2 line_amount_usd → ts.total_usd +
    // 2 lodging dates → per diem dates +
    // 3 conference → per diem (meals, start, end)
    expect(crossBranch.length).toBe(12);
  });
});

// ============================================================================
// Tier distribution tests
// ============================================================================

describe("Tier distribution", () => {
  it("has fields in all three tiers", () => {
    const tiers = new Set(Object.values(FIELDS).map((f) => f.source));
    expect(tiers).toContain("T1");
    expect(tiers).toContain("T2");
    expect(tiers).toContain("T3");
  });

  it("T1 fields do NOT have inferFrom (payee must supply them)", () => {
    const t1Fields = Object.entries(FIELDS).filter(
      ([, f]) => f.source === "T1"
    );
    expect(t1Fields.length).toBeGreaterThan(0);
    for (const [id, field] of t1Fields) {
      expect(field.inferFrom).toBeUndefined();
    }
  });

  it("T2 fields have system-oriented inferFrom descriptions", () => {
    const t2Fields = Object.entries(FIELDS).filter(
      ([, f]) => f.source === "T2"
    );
    expect(t2Fields.length).toBeGreaterThan(0);
    for (const [id, field] of t2Fields) {
      expect(field.inferFrom).toBeTruthy();
    }
  });

  it("T3 fields have document-extraction inferFrom descriptions", () => {
    const t3Fields = Object.entries(FIELDS).filter(
      ([, f]) => f.source === "T3"
    );
    expect(t3Fields.length).toBeGreaterThan(0);
    for (const [id, field] of t3Fields) {
      expect(field.inferFrom).toBeTruthy();
    }
  });
});

// ============================================================================
// Per-field tier correctness — verified against schema.yaml
// ============================================================================

describe("Field-to-tier assignments match schema.yaml", () => {
  // T1: Payee-provided — cannot be inferred or generated
  const expectedT1 = [
    "gi.authorized_by",
    "gi.rush_processing",
    "tla.source_documents",
    "tll.shared_with_txn",
    "tlgt.missing_receipt",
    "tlm.attendees",
    "tlm.meal_purpose",
    "tlgf.recipient_name",
    "tlgf.gift_purpose",
    "tlhs.irb_protocol_number",
    "aa.other_beneficiaries",
    "aa.beneficiary_list",
  ];

  // T2: System-generated — deterministic computation or API lookup
  const expectedT2 = [
    "gi.payment_method",
    "ts.total_usd",
    "ts.status",
    "tla.exchange_rate",
    "tll.exchange_rate",
    "tll.number_of_nights",
    "pd.number_of_days",
    "pd.per_diem_rate",
    "pd.meal_deductions",
    "pd.reimbursement_summary",
  ];

  // T3: System-inferred — extracted from documents by VLM/OCR
  const expectedT3 = [
    "gi.category",
    "gi.payee_name",
    "gi.payee_affiliation",
    "gi.business_purpose_where",
    "gi.business_purpose_when",
    "gi.business_purpose_why",
    "gi.event_name",
    "ts.transaction_type",
    "tla.date",
    "tla.original_currency",
    "tla.original_amount",
    "tla.line_amount_usd",
    "tla.expense_type",
    "tla.travelers_name",
    "tla.ticket_number",
    "tla.booking_method",
    "tla.destination_airport",
    "tla.class_of_ticket",
    "tll.original_amount",
    "tll.line_amount_usd",
    "tll.hotel_name",
    "tll.daily_rate",
    "tll.check_in_date",
    "tll.check_out_date",
    "tlc.conference_name",
    "tlc.conference_start_date",
    "tlc.conference_end_date",
    "tlc.meals_included",
    "tlgt.origin",
    "tlgt.destination",
    "tlm.venue_name",
    "tlr.rental_company",
    "tlr.rental_start_date",
    "tlhs.number_of_subjects",
    "pd.start_date",
    "pd.end_date",
    "pd.location",
  ];

  for (const id of expectedT1) {
    it(`${id} is T1 (payee-provided)`, () => {
      expect(FIELDS[id]).toBeDefined();
      expect(FIELDS[id].source).toBe("T1");
    });
  }

  for (const id of expectedT2) {
    it(`${id} is T2 (system-generated)`, () => {
      expect(FIELDS[id]).toBeDefined();
      expect(FIELDS[id].source).toBe("T2");
    });
  }

  for (const id of expectedT3) {
    it(`${id} is T3 (system-inferred)`, () => {
      expect(FIELDS[id]).toBeDefined();
      expect(FIELDS[id].source).toBe("T3");
    });
  }

  it("exhaustive: every field in FIELDS is covered by the tier lists above", () => {
    const allExpected = new Set([...expectedT1, ...expectedT2, ...expectedT3]);
    const allActual = new Set(Object.keys(FIELDS));
    expect(allExpected).toEqual(allActual);
  });
});

// ============================================================================
// Component render tests
// ============================================================================

describe("SchemaVisualizer component", () => {
  it("renders without crashing", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("Schema Visualizer")).toBeInTheDocument();
  });

  it("renders the subtitle", () => {
    render(<SchemaVisualizer />);
    expect(
      screen.getByText(
        "Stanford Expense Report · Workflow Schema Synthesis"
      )
    ).toBeInTheDocument();
  });

  it("renders all view mode buttons", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("Tree")).toBeInTheDocument();
    expect(screen.getByText("Graph")).toBeInTheDocument();
    expect(screen.getByText("Split")).toBeInTheDocument();
  });

  it("renders cross-branch filter button", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("cross-branch only")).toBeInTheDocument();
  });

  it("renders the legend with all tier labels", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("T1")).toBeInTheDocument();
    expect(screen.getByText("T2")).toBeInTheDocument();
    expect(screen.getByText("T3")).toBeInTheDocument();
  });

  it("renders the legend with all edge type labels", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("derive")).toBeInTheDocument();
    expect(screen.getByText("required-when")).toBeInTheDocument();
    expect(screen.getByText("soft flag")).toBeInTheDocument();
  });

  it("shows empty state in detail panel initially", () => {
    render(<SchemaVisualizer />);
    expect(screen.getByText("No field selected")).toBeInTheDocument();
  });

  it("renders section headers in tree view", () => {
    render(<SchemaVisualizer />);
    // Labels appear in both tree and graph (SVG), so use getAllByText
    expect(screen.getAllByText("General Information").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("Per Diem Expenses").length).toBeGreaterThanOrEqual(1);
  });

  it("shows field detail when a field is clicked in the tree", () => {
    render(<SchemaVisualizer />);
    // The tree is expanded by default, so "category" field label should be visible
    const categoryField = screen.getAllByText("category")[0];
    fireEvent.click(categoryField);
    // The detail panel should now show the description
    expect(
      screen.getByText(
        "Top-level expense category. Drives many conditional rules downstream."
      )
    ).toBeInTheDocument();
  });

  it("switching to tree-only view hides graph panel header", () => {
    render(<SchemaVisualizer />);
    // Click Tree button
    fireEvent.click(screen.getByText("Tree"));
    // "Dependency Graph" panel header should not be visible
    expect(screen.queryByText("Dependency Graph")).not.toBeInTheDocument();
  });

  it("switching to graph-only view hides tree panel header", () => {
    render(<SchemaVisualizer />);
    // Click Graph button
    fireEvent.click(screen.getByText("Graph"));
    // "Structural Tree" panel header should not be visible
    expect(screen.queryByText("Structural Tree")).not.toBeInTheDocument();
  });
});
