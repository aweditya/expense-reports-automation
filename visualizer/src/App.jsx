import React, { useState, useMemo, useCallback } from "react";
import { ChevronRight, ChevronDown, GitBranch, Network, Columns2, Filter, X } from "lucide-react";

// ============================================================================
// SCHEMA DATA — Stanford Expense Report (representative subset)
// ============================================================================
// Each field has: id, label, parent (structural), type, source tier (T1/T2/T3),
// required (boolean or expression), allowedValues, inferFrom, description.
// Edges are kept separately to make the dependency DAG explicit.
// ============================================================================

export const SECTIONS = [
  { id: "general_information", label: "General Information", row: 0 },
  { id: "transaction_summary", label: "Transaction Summary", row: 1 },
  { id: "tl_airfare", label: "Transaction Line · Airfare", row: 2, parent: "transaction_lines" },
  { id: "tl_lodging", label: "Transaction Line · Lodging", row: 3, parent: "transaction_lines" },
  { id: "tl_conference", label: "Transaction Line · Conference Registration", row: 4, parent: "transaction_lines" },
  { id: "tl_ground_transport", label: "Transaction Line · Ground Transport", row: 5, parent: "transaction_lines" },
  { id: "tl_meals", label: "Transaction Line · Meals", row: 6, parent: "transaction_lines" },
  { id: "tl_car_rental", label: "Transaction Line · Car Rental", row: 7, parent: "transaction_lines" },
  { id: "tl_gifts", label: "Transaction Line · Gifts", row: 8, parent: "transaction_lines" },
  { id: "tl_human_subjects", label: "Transaction Line · Human Subject Incentives", row: 9, parent: "transaction_lines" },
  { id: "per_diem", label: "Per Diem Expenses", row: 10 },
  { id: "allocation_and_approvers", label: "Allocation & Approvers", row: 11 },
];

export const FIELDS = {
  // ---- General Information ----
  "gi.category": {
    label: "category", parent: "general_information",
    type: "enum", source: "T3", required: true,
    allowedValues: ["expenses_domestic", "expenses_foreign", "athletic_use_only", "hr_use_only", "human_subjects", "relocation"],
    inferFrom: "Destination in flight/hotel docs. Foreign destination → expenses_foreign.",
    description: "Top-level expense category. Drives many conditional rules downstream."
  },
  "gi.payee_name": {
    label: "payee.name", parent: "general_information",
    type: "string", source: "T3", required: true,
    inferFrom: "Traveler name on flight booking or hotel folio.",
    description: "Name of the person being reimbursed."
  },
  "gi.payee_affiliation": {
    label: "payee.affiliation", parent: "general_information",
    type: "enum", source: "T3", required: true,
    allowedValues: ["stanford_student", "stanford_postdoc", "stanford_faculty", "stanford_staff", "other"],
    inferFrom: "Context from uploaded docs or FA input.",
    description: "Affiliation determines which student-certification options are valid."
  },
  "gi.payment_method": {
    label: "payment_method", parent: "general_information",
    type: "string", source: "T2", required: true,
    inferFrom: "Always 'electronic' — system pre-fills.",
    description: "Payment method. System always defaults to 'electronic'."
  },
  "gi.business_purpose_where": {
    label: "business_purpose.where", parent: "general_information",
    type: "string", source: "T3", required: true,
    inferFrom: "Destination city/country from flight or hotel docs.",
    description: "The 'where' component of the structured business purpose."
  },
  "gi.business_purpose_when": {
    label: "business_purpose.when", parent: "general_information",
    type: "string", source: "T3", required: true,
    inferFrom: "Trip dates from flight itinerary.",
    description: "The 'when' component of the structured business purpose."
  },
  "gi.business_purpose_why": {
    label: "business_purpose.why", parent: "general_information",
    type: "string", source: "T3", required: true,
    inferFrom: "Presenting at conference (if name appears in program), research collaboration, etc.",
    description: "The 'why' component of the structured business purpose."
  },
  "gi.event_name": {
    label: "event_name", parent: "general_information",
    type: "string", source: "T3", required: true,
    inferFrom: "Lab affiliation + category.",
    description: "Format: <lab_name> + <Foreign Expenses | Domestic Expenses>."
  },
  "gi.authorized_by": {
    label: "authorized_by", parent: "general_information",
    type: "string", source: "T1", required: true,
    description: "Faculty member or approver name. Only the payee or FA can supply this."
  },
  "gi.rush_processing": {
    label: "rush_processing", parent: "general_information",
    type: "enum", source: "T1", required: true,
    allowedValues: ["yes", "no"],
    description: "Whether the payee has explicitly requested expedited processing. Defaults to 'no'."
  },

  // ---- Transaction Summary ----
  "ts.transaction_type": {
    label: "transaction_type", parent: "transaction_summary",
    type: "enum", source: "T3", required: true,
    allowedValues: ["domestic", "foreign"],
    inferFrom: "Mirrors general_information.category.",
    description: "High-level classification mirroring the expense category."
  },
  "ts.total_usd": {
    label: "total_usd", parent: "transaction_summary",
    type: "number", source: "T2", required: true,
    inferFrom: "Sum of all transaction line amounts, converted to USD.",
    description: "Report grand total. Computed from all line_amount_usd fields."
  },
  "ts.status": {
    label: "status", parent: "transaction_summary",
    type: "enum", source: "T2",
    allowedValues: ["draft", "submitted", "approved", "returned", "paid"],
    inferFrom: "Workflow state tracked by the system.",
    description: "Current workflow state of the expense report."
  },

  // ---- Airfare ----
  "tla.date": {
    label: "common.date", parent: "tl_airfare",
    type: "date", source: "T3", required: true,
    inferFrom: "Date on the booking confirmation.",
    description: "Date of the airfare expense."
  },
  "tla.original_currency": {
    label: "common.original_currency", parent: "tl_airfare",
    type: "string", source: "T3",
    requiredWhen: "gi.category == expenses_foreign",
    inferFrom: "Currency symbol/code on receipt.",
    description: "Required only when the report category is foreign."
  },
  "tla.original_amount": {
    label: "common.original_amount", parent: "tl_airfare",
    type: "number", source: "T3", required: true,
    inferFrom: "Amount as printed on receipt.",
    description: "Amount in the original currency."
  },
  "tla.exchange_rate": {
    label: "common.exchange_rate", parent: "tl_airfare",
    type: "number", source: "T2",
    requiredWhen: "gi.category == expenses_foreign",
    inferFrom: "Historical rate API lookup for date + currency.",
    description: "System-generated. Looked up from an exchange rate API for the specific date."
  },
  "tla.line_amount_usd": {
    label: "common.line_amount_usd", parent: "tl_airfare",
    type: "number", source: "T3", required: true,
    inferFrom: "Receipt amount × exchange rate (if foreign).",
    description: "Amount in USD. If original currency is foreign, this is the converted amount."
  },
  "tla.expense_type": {
    label: "common.expense_type", parent: "tl_airfare",
    type: "enum", source: "T3", required: true,
    allowedValues: ["airfare_domestic", "airfare_foreign"],
    inferFrom: "LLM classifies from receipt content.",
    description: "Expense classification. Determines which detail block is populated."
  },
  "tla.source_documents": {
    label: "common.source_documents", parent: "tl_airfare",
    type: "array", source: "T1", required: true,
    description: "References to uploaded documents supporting this line. Only the payee can provide these."
  },
  "tla.travelers_name": {
    label: "airfare_details.travelers_name", parent: "tl_airfare",
    type: "string", source: "T3", required: true,
    inferFrom: "Booking confirmation.",
    description: "Traveler name as it appears on the booking."
  },
  "tla.ticket_number": {
    label: "airfare_details.ticket_number", parent: "tl_airfare",
    type: "string", source: "T3", required: true,
    inferFrom: "E-ticket receipt.",
    description: "Airline e-ticket number."
  },
  "tla.booking_method": {
    label: "airfare_details.booking_method", parent: "tl_airfare",
    type: "enum", source: "T3", required: true,
    allowedValues: ["stanford_travel_egencia", "stanford_travel_key_travel", "other"],
    inferFrom: "Booking confirmation format/header; default 'other' if unrecognized.",
    description: "How the flight was booked. Soft-flagged if not via Stanford Travel."
  },
  "tla.destination_airport": {
    label: "airfare_details.destination_airport", parent: "tl_airfare",
    type: "string", source: "T3", required: true,
    inferFrom: "Itinerary on booking confirmation.",
    description: "IATA airport code. Feeds into the business purpose 'where'."
  },
  "tla.class_of_ticket": {
    label: "airfare_details.class_of_ticket", parent: "tl_airfare",
    type: "enum", source: "T3", required: true,
    allowedValues: ["coach", "premium_economy", "business", "first"],
    inferFrom: "Booking confirmation.",
    description: "Class of ticket. Soft-flagged if above coach.",
    softFlags: ["class_above_coach_no_justification"]
  },

  // ---- Lodging ----
  "tll.original_amount": {
    label: "common.original_amount", parent: "tl_lodging",
    type: "number", source: "T3", required: true,
    inferFrom: "Hotel folio total.",
    description: "Amount in the original currency."
  },
  "tll.exchange_rate": {
    label: "common.exchange_rate", parent: "tl_lodging",
    type: "number", source: "T2",
    requiredWhen: "gi.category == expenses_foreign",
    inferFrom: "Historical rate API lookup for date + currency.",
    description: "System-generated currency conversion rate."
  },
  "tll.line_amount_usd": {
    label: "common.line_amount_usd", parent: "tl_lodging",
    type: "number", source: "T3", required: true,
    inferFrom: "Receipt amount × exchange rate (if foreign).",
    description: "Amount in USD. If original currency is foreign, this is the converted amount."
  },
  "tll.hotel_name": {
    label: "lodging_details.hotel_name", parent: "tl_lodging",
    type: "string", source: "T3", required: true,
    inferFrom: "Hotel folio header.",
    description: "Name of the hotel."
  },
  "tll.daily_rate": {
    label: "lodging_details.daily_rate", parent: "tl_lodging",
    type: "number", source: "T3", required: true,
    inferFrom: "Hotel folio.",
    description: "Nightly rate in original currency."
  },
  "tll.check_in_date": {
    label: "lodging_details.check_in_date", parent: "tl_lodging",
    type: "date", source: "T3", required: true,
    inferFrom: "Hotel folio.",
    description: "Hotel check-in. Used to derive number of nights AND per diem start date."
  },
  "tll.check_out_date": {
    label: "lodging_details.check_out_date", parent: "tl_lodging",
    type: "date", source: "T3", required: true,
    inferFrom: "Hotel folio.",
    description: "Hotel check-out."
  },
  "tll.number_of_nights": {
    label: "lodging_details.number_of_nights", parent: "tl_lodging",
    type: "number", source: "T2", required: true,
    inferFrom: "Computed: check_out_date − check_in_date.",
    description: "Derived field. Auto-computed."
  },
  "tll.shared_with_txn": {
    label: "lodging_details.shared_with_transaction_number", parent: "tl_lodging",
    type: "string", source: "T1",
    description: "ERxxxxxxx of the other traveler's report. Required when lodging is shared. Only the payee knows."
  },

  // ---- Conference Registration ----
  "tlc.conference_name": {
    label: "conference_registration_details.conference_name", parent: "tl_conference",
    type: "string", source: "T3", required: true,
    inferFrom: "Registration receipt.",
    description: "Name of the conference."
  },
  "tlc.conference_start_date": {
    label: "conference_registration_details.start_date", parent: "tl_conference",
    type: "date", source: "T3", required: true,
    inferFrom: "Conference program.",
    description: "First day of the conference."
  },
  "tlc.conference_end_date": {
    label: "conference_registration_details.end_date", parent: "tl_conference",
    type: "date", source: "T3", required: true,
    inferFrom: "Conference program.",
    description: "Last day of the conference."
  },
  "tlc.meals_included": {
    label: "conference_registration_details.meals_included", parent: "tl_conference",
    type: "object", source: "T3", required: true,
    inferFrom: "Conference program/schedule (e.g., 'lunch provided to all attendees').",
    description: "Per-day breakdown of meals provided by the conference. Drives per diem deductions."
  },

  // ---- Ground Transport ----
  "tlgt.origin": {
    label: "ground_transport_details.origin", parent: "tl_ground_transport",
    type: "string", source: "T3", required: true,
    inferFrom: "Uber/Lyft receipt.",
    description: "Pickup location."
  },
  "tlgt.destination": {
    label: "ground_transport_details.destination", parent: "tl_ground_transport",
    type: "string", source: "T3", required: true,
    inferFrom: "Uber/Lyft receipt.",
    description: "Drop-off location."
  },
  "tlgt.missing_receipt": {
    label: "ground_transport_details.missing_receipt", parent: "tl_ground_transport",
    type: "boolean", source: "T1", required: true,
    description: "If true, missing receipt form is used instead. Only the payee can confirm this."
  },

  // ---- Meals ----
  "tlm.venue_name": {
    label: "meal_details.venue_name", parent: "tl_meals",
    type: "string", source: "T3", required: true,
    inferFrom: "Receipt header.",
    description: "Name of the restaurant or venue."
  },
  "tlm.attendees": {
    label: "meal_details.attendees", parent: "tl_meals",
    type: "array", source: "T1", required: true,
    description: "List of attendees with names and affiliations. Only the payee knows who attended."
  },
  "tlm.meal_purpose": {
    label: "meal_details.meal_purpose", parent: "tl_meals",
    type: "string", source: "T1", required: true,
    description: "Business purpose of the meal. Must be provided by the payee."
  },

  // ---- Car Rental ----
  "tlr.rental_company": {
    label: "car_rental_details.rental_company", parent: "tl_car_rental",
    type: "string", source: "T3", required: true,
    inferFrom: "Rental agreement.",
    description: "Name of the rental company."
  },
  "tlr.rental_start_date": {
    label: "car_rental_details.rental_start_date", parent: "tl_car_rental",
    type: "date", source: "T3", required: true,
    inferFrom: "Rental agreement.",
    description: "Start date of the rental period."
  },

  // ---- Gifts ----
  "tlgf.recipient_name": {
    label: "gift_details.recipient_name", parent: "tl_gifts",
    type: "string", source: "T1", required: true,
    description: "Name of the gift recipient. Only the payee can supply this."
  },
  "tlgf.gift_purpose": {
    label: "gift_details.gift_purpose", parent: "tl_gifts",
    type: "string", source: "T1", required: true,
    description: "Business reason for the gift. Must be provided by the payee."
  },

  // ---- Human Subject Incentives ----
  "tlhs.irb_protocol_number": {
    label: "human_subject_details.irb_protocol_number", parent: "tl_human_subjects",
    type: "string", source: "T1", required: true,
    description: "IRB protocol number. Must be provided by the payee."
  },
  "tlhs.number_of_subjects": {
    label: "human_subject_details.number_of_subjects", parent: "tl_human_subjects",
    type: "number", source: "T3", required: true,
    inferFrom: "Distribution log.",
    description: "Number of subjects who received incentives."
  },

  // ---- Per Diem ----
  "pd.start_date": {
    label: "start_date", parent: "per_diem",
    type: "date", source: "T3", required: true,
    inferFrom: "Trip start from flight itinerary OR conference start date.",
    description: "First day of per diem coverage."
  },
  "pd.end_date": {
    label: "end_date", parent: "per_diem",
    type: "date", source: "T3", required: true,
    inferFrom: "Trip end from flight itinerary OR conference end date.",
    description: "Last day of per diem coverage."
  },
  "pd.number_of_days": {
    label: "number_of_days", parent: "per_diem",
    type: "number", source: "T2", required: true,
    inferFrom: "Computed: end_date − start_date + 1.",
    description: "Total days of per diem coverage. Auto-computed."
  },
  "pd.location": {
    label: "location", parent: "per_diem",
    type: "string", source: "T3", required: true,
    inferFrom: "Destination from flight or hotel docs.",
    description: "City, State/Country for per diem rate lookup."
  },
  "pd.per_diem_rate": {
    label: "per_diem_rate", parent: "per_diem",
    type: "number", source: "T2", required: true,
    inferFrom: "GSA (domestic) or State Dept (foreign) API lookup by location + date.",
    description: "System-generated daily rate."
  },
  "pd.meal_deductions": {
    label: "meal_deductions", parent: "per_diem",
    type: "array", source: "T2", required: true,
    inferFrom: "Cross-reference conference meals_included with trip dates.",
    description: "Pre-filled deduction grid. The cross-branch derivation is the whole point."
  },
  "pd.reimbursement_summary": {
    label: "reimbursement_summary", parent: "per_diem",
    type: "array", source: "T2", required: true,
    inferFrom: "Computed: per_diem_rate − meal_deductions per day.",
    description: "Final per-day reimbursement table."
  },

  // ---- Allocation & Approvers ----
  "aa.other_beneficiaries": {
    label: "other_beneficiaries", parent: "allocation_and_approvers",
    type: "boolean", source: "T1", required: true,
    description: "Are there any beneficiaries other than the payee? Only the payee knows."
  },
  "aa.beneficiary_list": {
    label: "beneficiary_list", parent: "allocation_and_approvers",
    type: "array", source: "T1",
    requiredWhen: "aa.other_beneficiaries == true",
    description: "List of other beneficiaries with names and relationships."
  },
};

// ---- Edges (the dependency DAG) ----
// type: derive | required-when | flag
// crossBranch is computed by checking if from/to live in different top-level sections.
export const RAW_EDGES = [
  // Within general information
  { from: "gi.category", to: "gi.event_name", type: "derive", note: "Category determines domestic/foreign suffix" },
  { from: "gi.category", to: "ts.transaction_type", type: "derive", note: "Mirrors category" },
  { from: "gi.payee_name", to: "gi.business_purpose_why", type: "derive", note: "Name matched against conference program" },

  // Within airfare line
  { from: "tla.original_amount", to: "tla.line_amount_usd", type: "derive", note: "× exchange_rate" },
  { from: "tla.exchange_rate", to: "tla.line_amount_usd", type: "derive", note: "× original_amount" },
  { from: "tla.date", to: "tla.exchange_rate", type: "derive", note: "Rate looked up for this date" },
  { from: "tla.original_currency", to: "tla.exchange_rate", type: "derive", note: "Rate for this currency" },

  // Within lodging line
  { from: "tll.original_amount", to: "tll.line_amount_usd", type: "derive", note: "× exchange_rate" },
  { from: "tll.exchange_rate", to: "tll.line_amount_usd", type: "derive", note: "× original_amount" },
  { from: "tll.check_in_date", to: "tll.number_of_nights", type: "derive", note: "− check_out_date" },
  { from: "tll.check_out_date", to: "tll.number_of_nights", type: "derive", note: "− check_in_date" },

  // Within per diem
  { from: "pd.start_date", to: "pd.number_of_days", type: "derive", note: "end_date − start_date + 1" },
  { from: "pd.end_date", to: "pd.number_of_days", type: "derive", note: "end_date − start_date + 1" },
  { from: "pd.start_date", to: "pd.reimbursement_summary", type: "derive" },
  { from: "pd.end_date", to: "pd.reimbursement_summary", type: "derive" },
  { from: "pd.per_diem_rate", to: "pd.reimbursement_summary", type: "derive" },
  { from: "pd.meal_deductions", to: "pd.reimbursement_summary", type: "derive" },
  { from: "pd.location", to: "pd.per_diem_rate", type: "derive", note: "Rate looked up by location" },

  // Within allocation & approvers
  { from: "aa.other_beneficiaries", to: "aa.beneficiary_list", type: "required-when", predicate: "other_beneficiaries == true" },

  // Cross-branch: required-when from category
  { from: "gi.category", to: "tla.original_currency", type: "required-when", predicate: "category == expenses_foreign" },
  { from: "gi.category", to: "tla.exchange_rate", type: "required-when", predicate: "category == expenses_foreign" },
  { from: "gi.category", to: "tll.exchange_rate", type: "required-when", predicate: "category == expenses_foreign" },

  // Cross-branch: airfare destination → business purpose where
  { from: "tla.destination_airport", to: "gi.business_purpose_where", type: "derive", note: "Geocode airport → city/country" },

  // Cross-branch: line amounts → transaction summary total
  { from: "tla.line_amount_usd", to: "ts.total_usd", type: "derive", note: "Summed into report total" },
  { from: "tll.line_amount_usd", to: "ts.total_usd", type: "derive", note: "Summed into report total" },

  // Cross-branch: lodging dates → per diem dates
  { from: "tll.check_in_date", to: "pd.start_date", type: "derive", note: "Default to hotel check-in" },
  { from: "tll.check_out_date", to: "pd.end_date", type: "derive", note: "Default to hotel check-out" },

  // Cross-branch: conference → per diem (THE STAR)
  { from: "tlc.meals_included", to: "pd.meal_deductions", type: "derive", note: "★ Cross-branch: conference meals drive deductions" },
  { from: "tlc.conference_start_date", to: "pd.start_date", type: "derive", note: "Alternative source for per diem start" },
  { from: "tlc.conference_end_date", to: "pd.end_date", type: "derive", note: "Alternative source for per diem end" },

  // Soft constraint flag
  { from: "tla.class_of_ticket", to: "tla.class_of_ticket", type: "flag", note: "Soft flag if above coach", selfLoop: true },
];

// Helper: section lookup
export const SECTION_BY_ID = Object.fromEntries(SECTIONS.map(s => [s.id, s]));
export const FIELD_TO_TOP_SECTION = (fieldId) => {
  const parent = FIELDS[fieldId]?.parent;
  if (!parent) return null;
  const section = SECTION_BY_ID[parent];
  return section?.parent || section?.id || parent;
};

export const EDGES = RAW_EDGES.map(e => ({
  ...e,
  crossBranch: e.selfLoop ? false : FIELD_TO_TOP_SECTION(e.from) !== FIELD_TO_TOP_SECTION(e.to),
}));

// ============================================================================
// Tier styling
// ============================================================================
const TIER_STYLES = {
  T1: { color: "#c8553d", bg: "#f5d8d0", label: "Payee-provided" },
  T2: { color: "#2c6e7f", bg: "#cfe2e6", label: "System-generated" },
  T3: { color: "#b08642", bg: "#ecdcb8", label: "System-inferred" },
};

const EDGE_STYLES = {
  derive: { color: "#5a5a5a", dash: "none", label: "derive" },
  "required-when": { color: "#8b2a3e", dash: "6 4", label: "required-when" },
  flag: { color: "#b08642", dash: "2 3", label: "soft flag" },
};

// ============================================================================
// Tree View
// ============================================================================
function TreeView({ selected, setSelected, hover, setHover }) {
  const [expanded, setExpanded] = useState(new Set([
    "general_information", "transaction_lines", "tl_airfare", "tl_lodging", "tl_conference", "per_diem"
  ]));

  const toggle = (id) => {
    const next = new Set(expanded);
    if (next.has(id)) next.delete(id); else next.add(id);
    setExpanded(next);
  };

  // Build hierarchy
  const topSections = SECTIONS.filter(s => !s.parent);

  const fieldsByParent = useMemo(() => {
    const map = {};
    Object.entries(FIELDS).forEach(([id, f]) => {
      if (!map[f.parent]) map[f.parent] = [];
      map[f.parent].push({ id, ...f });
    });
    return map;
  }, []);

  const renderField = (id, field) => {
    const tier = TIER_STYLES[field.source];
    const isSelected = selected === id;
    const isHovered = hover === id;
    return (
      <div
        key={id}
        onClick={() => setSelected(id)}
        onMouseEnter={() => setHover(id)}
        onMouseLeave={() => setHover(null)}
        className="group flex items-center gap-2 py-1 px-2 ml-6 cursor-pointer rounded transition-all"
        style={{
          background: isSelected ? "#1a1d29" : isHovered ? "#ebe4d3" : "transparent",
          color: isSelected ? "#f5f0e6" : "#1a1d29",
        }}
      >
        <span
          className="inline-block w-2 h-2 rounded-full shrink-0"
          style={{ background: tier?.color || "#888" }}
        />
        <span className="font-mono text-xs truncate">{field.label}</span>
        <span
          className="ml-auto font-mono text-[10px] uppercase tracking-wider opacity-60 shrink-0"
          style={{ color: isSelected ? tier?.bg : tier?.color }}
        >
          {field.source} · {field.type}
        </span>
      </div>
    );
  };

  const renderSection = (section, depth = 0) => {
    const isOpen = expanded.has(section.id);
    const childSubs = SECTIONS.filter(s => s.parent === section.id);
    const childFields = fieldsByParent[section.id] || [];
    return (
      <div key={section.id}>
        <div
          onClick={() => toggle(section.id)}
          className="flex items-center gap-1 py-1.5 px-2 cursor-pointer hover:bg-[#ebe4d3] rounded transition-colors"
          style={{ marginLeft: depth * 12 }}
        >
          {isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <span className="font-serif text-sm tracking-tight" style={{ fontStyle: depth > 0 ? "italic" : "normal" }}>
            {section.label}
          </span>
        </div>
        {isOpen && (
          <div>
            {childSubs.map(s => renderSection(s, depth + 1))}
            {childFields.map(f => renderField(f.id, f))}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="h-full overflow-auto px-3 py-4 pb-10">
      {topSections.map(s => renderSection(s, 0))}
    </div>
  );
}

// ============================================================================
// Graph View
// ============================================================================
function GraphView({ selected, setSelected, hover, setHover, crossBranchOnly }) {
  // Layout: each section is a horizontal row. Fields are placed in columns within rows.
  const layout = useMemo(() => {
    const fieldsByLayoutSection = {};
    const PADDING_X = 32;
    const PADDING_Y = 24;
    const FIELD_W = 178;
    const FIELD_H = 44;
    const FIELD_GAP_X = 14;
    const FIELD_GAP_Y = 12;
    const ROW_HEADER_H = 28;
    const ROW_GAP = 22;

    // Group fields by their immediate parent (which is one of the SECTIONS rows)
    Object.entries(FIELDS).forEach(([id, f]) => {
      const sec = f.parent;
      if (!fieldsByLayoutSection[sec]) fieldsByLayoutSection[sec] = [];
      fieldsByLayoutSection[sec].push(id);
    });

    // Compute row dimensions
    const rows = SECTIONS.filter(s => fieldsByLayoutSection[s.id]).sort((a, b) => a.row - b.row);

    let cursorY = PADDING_Y;
    const positions = {};
    const rowBoxes = [];
    const COLS_PER_ROW = 6;

    rows.forEach((section) => {
      const fieldIds = fieldsByLayoutSection[section.id] || [];
      const numCols = Math.min(COLS_PER_ROW, fieldIds.length);
      const numRows = Math.ceil(fieldIds.length / numCols);
      const rowHeight = ROW_HEADER_H + numRows * (FIELD_H + FIELD_GAP_Y);
      const rowWidth = PADDING_X * 2 + numCols * (FIELD_W + FIELD_GAP_X) - FIELD_GAP_X;

      rowBoxes.push({
        id: section.id,
        label: section.label,
        x: PADDING_X / 2,
        y: cursorY,
        width: rowWidth,
        height: rowHeight,
      });

      fieldIds.forEach((fid, idx) => {
        const col = idx % numCols;
        const row = Math.floor(idx / numCols);
        const x = PADDING_X + col * (FIELD_W + FIELD_GAP_X);
        const y = cursorY + ROW_HEADER_H + row * (FIELD_H + FIELD_GAP_Y);
        positions[fid] = { x, y, width: FIELD_W, height: FIELD_H };
      });

      cursorY += rowHeight + ROW_GAP;
    });

    const totalWidth = Math.max(...rowBoxes.map(r => r.x + r.width)) + PADDING_X / 2;
    const totalHeight = cursorY;

    return { positions, rowBoxes, totalWidth, totalHeight };
  }, []);

  const visibleEdges = useMemo(() => {
    return EDGES.filter(e => {
      if (e.selfLoop) return false;
      if (crossBranchOnly && !e.crossBranch) return false;
      return true;
    });
  }, [crossBranchOnly]);

  const focused = hover || selected;

  const buildPath = (fromPos, toPos) => {
    const fromCx = fromPos.x + fromPos.width / 2;
    const fromCy = fromPos.y + fromPos.height;
    const toCx = toPos.x + toPos.width / 2;
    const toCy = toPos.y;
    // Going up (toCy < fromCy): flip control points
    if (toCy < fromCy) {
      const fromCyTop = fromPos.y;
      const toCyBot = toPos.y + toPos.height;
      const dy2 = Math.abs(fromCyTop - toCyBot);
      return `M ${fromCx} ${fromCyTop} C ${fromCx} ${fromCyTop - Math.max(40, dy2 * 0.4)}, ${toCx} ${toCyBot + Math.max(40, dy2 * 0.4)}, ${toCx} ${toCyBot}`;
    }
    const dy = Math.abs(toCy - fromCy);
    const c1y = fromCy + Math.max(40, dy * 0.4);
    const c2y = toCy - Math.max(40, dy * 0.4);
    return `M ${fromCx} ${fromCy} C ${fromCx} ${c1y}, ${toCx} ${c2y}, ${toCx} ${toCy}`;
  };

  return (
    <div className="h-full overflow-auto bg-[#faf6ec] pb-10">
      <svg
        width={layout.totalWidth}
        height={layout.totalHeight}
        style={{ display: "block" }}
      >
        <defs>
          {Object.entries(EDGE_STYLES).map(([type, style]) => (
            <marker
              key={type}
              id={`arrow-${type}`}
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="5"
              markerHeight="5"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill={style.color} />
            </marker>
          ))}
          {Object.entries(EDGE_STYLES).map(([type, style]) => (
            <marker
              key={`${type}-bold`}
              id={`arrow-${type}-bold`}
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill={style.color} />
            </marker>
          ))}
        </defs>

        {/* Section row backgrounds */}
        {layout.rowBoxes.map((row) => (
          <g key={row.id}>
            <rect
              x={row.x}
              y={row.y}
              width={row.width}
              height={row.height}
              fill="#f0e9d6"
              stroke="#d8cdb0"
              strokeWidth="1"
              rx="3"
            />
            <text
              x={row.x + 14}
              y={row.y + 18}
              fontFamily="'Newsreader', serif"
              fontSize="13"
              fontStyle="italic"
              fill="#5a4f38"
            >
              {row.label}
            </text>
          </g>
        ))}

        {/* Edges */}
        {visibleEdges.map((edge, i) => {
          const fromPos = layout.positions[edge.from];
          const toPos = layout.positions[edge.to];
          if (!fromPos || !toPos) return null;
          const style = EDGE_STYLES[edge.type];
          const isHighlighted = focused && (edge.from === focused || edge.to === focused);
          const isDimmed = focused && !isHighlighted;
          const isCross = edge.crossBranch;
          return (
            <path
              key={i}
              d={buildPath(fromPos, toPos)}
              fill="none"
              stroke={style.color}
              strokeWidth={isHighlighted ? 2.4 : isCross ? 1.6 : 1}
              strokeDasharray={style.dash}
              opacity={isDimmed ? 0.12 : isHighlighted ? 1 : isCross ? 0.85 : 0.55}
              markerEnd={`url(#arrow-${edge.type}${isHighlighted ? "-bold" : ""})`}
              style={{ transition: "opacity 0.15s, stroke-width 0.15s" }}
            />
          );
        })}

        {/* Field nodes */}
        {Object.entries(layout.positions).map(([id, pos]) => {
          const field = FIELDS[id];
          const tier = TIER_STYLES[field.source];
          const isSelected = selected === id;
          const isHovered = hover === id;
          const isInFocus = focused && (
            id === focused ||
            visibleEdges.some(e => (e.from === focused && e.to === id) || (e.to === focused && e.from === id))
          );
          const isDimmed = focused && !isInFocus;

          return (
            <g
              key={id}
              transform={`translate(${pos.x}, ${pos.y})`}
              onClick={() => setSelected(id)}
              onMouseEnter={() => setHover(id)}
              onMouseLeave={() => setHover(null)}
              style={{ cursor: "pointer", opacity: isDimmed ? 0.3 : 1, transition: "opacity 0.15s" }}
            >
              <rect
                width={pos.width}
                height={pos.height}
                fill={isSelected ? "#1a1d29" : "#fdfbf3"}
                stroke={isSelected ? "#1a1d29" : isHovered ? tier.color : "#c5b896"}
                strokeWidth={isSelected || isHovered ? 1.5 : 1}
                rx="2"
              />
              <rect
                width="3"
                height={pos.height}
                fill={tier.color}
                rx="1"
              />
              <text
                x="11"
                y="17"
                fontFamily="'JetBrains Mono', monospace"
                fontSize="10.5"
                fill={isSelected ? "#f5f0e6" : "#1a1d29"}
                fontWeight="500"
              >
                {field.label.length > 26 ? field.label.slice(0, 24) + "…" : field.label}
              </text>
              <text
                x="11"
                y="32"
                fontFamily="'IBM Plex Sans', sans-serif"
                fontSize="9"
                fill={isSelected ? tier.bg : tier.color}
                letterSpacing="0.05em"
              >
                {field.source} · {field.type}
              </text>
              {field.requiredWhen && (
                <text
                  x={pos.width - 7}
                  y="14"
                  fontFamily="'IBM Plex Sans', sans-serif"
                  fontSize="8"
                  fill={isSelected ? "#f5d8d0" : "#8b2a3e"}
                  textAnchor="end"
                  fontStyle="italic"
                >
                  cond
                </text>
              )}
              {field.softFlags && (
                <text
                  x={pos.width - 7}
                  y="14"
                  fontFamily="'IBM Plex Sans', sans-serif"
                  fontSize="8"
                  fill={isSelected ? "#ecdcb8" : "#b08642"}
                  textAnchor="end"
                  fontStyle="italic"
                >
                  flag
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </div>
  );
}

// ============================================================================
// Detail Panel
// ============================================================================
function DetailPanel({ selected, setSelected }) {
  if (!selected) {
    return (
      <div className="h-full p-6 flex flex-col">
        <div className="border-b border-[#d8cdb0] pb-3 mb-4">
          <p className="font-serif text-sm italic text-[#5a4f38]">No field selected</p>
        </div>
        <div className="text-xs text-[#5a4f38] space-y-3 leading-relaxed">
          <p>Click any field in the tree or graph to inspect it.</p>
          <p>The schema below describes a representative subset of the Stanford expense report workflow.</p>
          <div className="pt-4 border-t border-[#d8cdb0] mt-6">
            <p className="font-serif italic text-sm mb-2">What to look for</p>
            <ul className="space-y-2 list-none">
              <li>— Tier color stripe shows whether the field is payee-provided (T1), system-generated (T2), or system-inferred (T3).</li>
              <li>— Solid edges denote derive relationships; dashed magenta edges denote conditional requirements.</li>
              <li>— Cross-branch edges (those that cross row boundaries in the graph view) are the visible evidence that the dependency DAG is not the structural tree.</li>
            </ul>
          </div>
        </div>
      </div>
    );
  }

  const field = FIELDS[selected];
  const tier = TIER_STYLES[field.source];
  const incoming = EDGES.filter(e => e.to === selected && !e.selfLoop);
  const outgoing = EDGES.filter(e => e.from === selected && !e.selfLoop);
  const parentSection = SECTION_BY_ID[field.parent];

  return (
    <div className="h-full overflow-auto">
      <div className="p-6 pb-10">
        <div className="flex items-start justify-between mb-1">
          <p className="font-serif italic text-xs text-[#5a4f38] tracking-wide">
            {parentSection?.label}
          </p>
          <button
            onClick={() => setSelected(null)}
            className="text-[#5a4f38] hover:text-[#1a1d29] transition-colors -mt-1"
          >
            <X size={14} />
          </button>
        </div>
        <p className="font-mono text-[15px] text-[#1a1d29] mb-4 break-all leading-tight">{field.label}</p>

        <div className="flex items-center gap-2 mb-5">
          <span
            className="px-2 py-0.5 text-[10px] font-mono tracking-wider uppercase rounded-sm"
            style={{ background: tier.color, color: "#faf6ec" }}
          >
            {field.source}
          </span>
          <span className="text-[10px] font-mono uppercase tracking-wider text-[#5a4f38]">
            {tier.label}
          </span>
        </div>

        <p className="text-sm text-[#1a1d29] leading-relaxed mb-5 italic font-serif">
          {field.description}
        </p>

        <div className="space-y-4 text-xs">
          <PropRow label="type" value={field.type} mono />
          <PropRow
            label="required"
            value={field.required === true ? "always" : field.requiredWhen ? `when: ${field.requiredWhen}` : "no"}
            mono={!!field.requiredWhen}
            accent={field.requiredWhen ? "#8b2a3e" : null}
          />
          {field.allowedValues && (
            <div>
              <p className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] mb-1.5">allowed_values</p>
              <div className="flex flex-wrap gap-1">
                {field.allowedValues.map(v => (
                  <span key={v} className="px-1.5 py-0.5 text-[10px] font-mono bg-[#ebe4d3] text-[#1a1d29] rounded-sm">
                    {v}
                  </span>
                ))}
              </div>
            </div>
          )}
          {field.inferFrom && (
            <div>
              <p className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] mb-1.5">infer_from</p>
              <p className="font-serif italic text-[#1a1d29] leading-snug">{field.inferFrom}</p>
            </div>
          )}
          {field.softFlags && (
            <div>
              <p className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] mb-1.5">soft_flags</p>
              {field.softFlags.map(f => (
                <p key={f} className="font-mono text-[11px] text-[#b08642]">{f}</p>
              ))}
            </div>
          )}
        </div>

        {(incoming.length > 0 || outgoing.length > 0) && (
          <div className="mt-7 pt-5 border-t border-[#d8cdb0]">
            <p className="font-serif italic text-sm text-[#1a1d29] mb-3">Dependencies</p>
            {incoming.length > 0 && (
              <div className="mb-4">
                <p className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] mb-2">depends on (incoming)</p>
                <div className="space-y-1.5">
                  {incoming.map((e, i) => (
                    <EdgeRow key={i} edge={e} side="from" />
                  ))}
                </div>
              </div>
            )}
            {outgoing.length > 0 && (
              <div>
                <p className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] mb-2">feeds into (outgoing)</p>
                <div className="space-y-1.5">
                  {outgoing.map((e, i) => (
                    <EdgeRow key={i} edge={e} side="to" />
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function PropRow({ label, value, mono, accent }) {
  return (
    <div className="flex items-baseline gap-3">
      <span className="font-mono text-[10px] uppercase tracking-wider text-[#5a4f38] shrink-0 w-20">{label}</span>
      <span
        className={mono ? "font-mono text-xs" : "text-xs"}
        style={{ color: accent || "#1a1d29" }}
      >
        {value}
      </span>
    </div>
  );
}

function EdgeRow({ edge, side }) {
  const fieldId = side === "from" ? edge.from : edge.to;
  const field = FIELDS[fieldId];
  const style = EDGE_STYLES[edge.type];
  return (
    <div className="flex items-start gap-2 text-[11px]">
      <span
        className="font-mono text-[9px] uppercase tracking-wider px-1 py-0.5 rounded-sm shrink-0 mt-0.5"
        style={{ color: style.color, background: edge.crossBranch ? "#fae5d0" : "transparent", border: `1px solid ${style.color}` }}
      >
        {edge.type}{edge.crossBranch ? " · cross" : ""}
      </span>
      <div className="flex-1 min-w-0">
        <p className="font-mono text-xs text-[#1a1d29] truncate">{field?.label || fieldId}</p>
        {edge.note && <p className="text-[10px] italic text-[#5a4f38] mt-0.5">{edge.note}</p>}
        {edge.predicate && <p className="font-mono text-[10px] text-[#8b2a3e] mt-0.5">{edge.predicate}</p>}
      </div>
    </div>
  );
}

// ============================================================================
// Legend
// ============================================================================
function Legend() {
  return (
    <div className="flex items-center gap-6 px-5 py-2.5 border-t border-[#d8cdb0] bg-[#f0e9d6] text-[10px] font-mono uppercase tracking-wider text-[#5a4f38]">
      <div className="flex items-center gap-3">
        <span className="font-serif italic normal-case text-[11px] tracking-normal text-[#1a1d29]">Tier</span>
        {Object.entries(TIER_STYLES).map(([k, s]) => (
          <span key={k} className="flex items-center gap-1.5">
            <span className="inline-block w-2.5 h-2.5 rounded-full" style={{ background: s.color }} />
            <span>{k}</span>
          </span>
        ))}
      </div>
      <div className="w-px h-4 bg-[#d8cdb0]" />
      <div className="flex items-center gap-3">
        <span className="font-serif italic normal-case text-[11px] tracking-normal text-[#1a1d29]">Edge</span>
        {Object.entries(EDGE_STYLES).map(([k, s]) => (
          <span key={k} className="flex items-center gap-1.5">
            <svg width="20" height="6">
              <line x1="0" y1="3" x2="20" y2="3" stroke={s.color} strokeWidth="1.5" strokeDasharray={s.dash} />
            </svg>
            <span>{s.label}</span>
          </span>
        ))}
      </div>
      <div className="w-px h-4 bg-[#d8cdb0]" />
      <span className="font-serif italic normal-case text-[11px] tracking-normal text-[#1a1d29]">
        edges crossing row boundaries are cross-branch dependencies
      </span>
    </div>
  );
}

// ============================================================================
// Main App
// ============================================================================
export default function SchemaVisualizer() {
  const [view, setView] = useState("split"); // "tree" | "graph" | "split"
  const [selected, setSelected] = useState(null);
  const [hover, setHover] = useState(null);
  const [crossBranchOnly, setCrossBranchOnly] = useState(false);

  const ViewButton = ({ value, label, icon: Icon }) => (
    <button
      onClick={() => setView(value)}
      className="flex items-center gap-1.5 px-2.5 py-1 text-[11px] font-mono uppercase tracking-wider transition-colors rounded-sm"
      style={{
        background: view === value ? "#1a1d29" : "transparent",
        color: view === value ? "#f5f0e6" : "#5a4f38",
      }}
    >
      <Icon size={12} />
      {label}
    </button>
  );

  return (
    <div
      className="w-full h-screen flex flex-col"
      style={{
        background: "#f5f0e6",
        fontFamily: "'IBM Plex Sans', system-ui, sans-serif",
      }}
    >
      <link href="https://fonts.googleapis.com/css2?family=Newsreader:ital,wght@0,400;0,500;1,400;1,500&family=IBM+Plex+Sans:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet" />

      {/* Header */}
      <header className="border-b border-[#d8cdb0] px-6 py-3 flex items-center justify-between bg-[#faf6ec]">
        <div>
          <h1 className="font-serif italic text-2xl text-[#1a1d29] leading-none tracking-tight">
            Schema Visualizer
          </h1>
          <p className="text-[10px] font-mono uppercase tracking-[0.15em] text-[#5a4f38] mt-1">
            Stanford Expense Report · Workflow Schema Synthesis
          </p>
        </div>
        <div className="flex items-center gap-3">
          <button
            onClick={() => setCrossBranchOnly(!crossBranchOnly)}
            className="flex items-center gap-1.5 px-2.5 py-1 text-[11px] font-mono uppercase tracking-wider transition-colors rounded-sm border"
            style={{
              background: crossBranchOnly ? "#8b2a3e" : "transparent",
              color: crossBranchOnly ? "#f5f0e6" : "#5a4f38",
              borderColor: crossBranchOnly ? "#8b2a3e" : "#d8cdb0",
            }}
          >
            <Filter size={12} />
            cross-branch only
          </button>
          <div className="flex items-center bg-[#ebe4d3] rounded-sm p-0.5">
            <ViewButton value="tree" label="Tree" icon={GitBranch} />
            <ViewButton value="graph" label="Graph" icon={Network} />
            <ViewButton value="split" label="Split" icon={Columns2} />
          </div>
        </div>
      </header>

      {/* Main content area */}
      <div className="flex-1 flex overflow-hidden">
        {(view === "tree" || view === "split") && (
          <div
            className="border-r border-[#d8cdb0] bg-[#faf6ec] overflow-hidden"
            style={{ width: view === "split" ? "300px" : "100%", flexShrink: 0 }}
          >
            <div className="px-3 py-2 border-b border-[#d8cdb0] bg-[#f0e9d6]">
              <p className="font-serif italic text-xs text-[#5a4f38]">Structural Tree</p>
            </div>
            <TreeView
              selected={selected}
              setSelected={setSelected}
              hover={hover}
              setHover={setHover}
            />
          </div>
        )}
        {(view === "graph" || view === "split") && (
          <div className="flex-1 overflow-hidden flex flex-col">
            <div className="px-3 py-2 border-b border-[#d8cdb0] bg-[#f0e9d6] flex items-center justify-between">
              <p className="font-serif italic text-xs text-[#5a4f38]">Dependency Graph</p>
              {selected && (
                <p className="text-[10px] font-mono text-[#5a4f38]">
                  showing edges incident to <span className="text-[#1a1d29]">{FIELDS[selected]?.label}</span>
                </p>
              )}
            </div>
            <div className="flex-1 overflow-hidden">
              <GraphView
                selected={selected}
                setSelected={setSelected}
                hover={hover}
                setHover={setHover}
                crossBranchOnly={crossBranchOnly}
              />
            </div>
          </div>
        )}
        <div
          className="border-l border-[#d8cdb0] bg-[#faf6ec] overflow-hidden"
          style={{ width: "340px", flexShrink: 0 }}
        >
          <div className="px-3 py-2 border-b border-[#d8cdb0] bg-[#f0e9d6]">
            <p className="font-serif italic text-xs text-[#5a4f38]">Field Detail</p>
          </div>
          <DetailPanel selected={selected} setSelected={setSelected} />
        </div>
      </div>

      <Legend />
    </div>
  );
}
