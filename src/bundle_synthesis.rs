use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::document_facts::{
    DateRange, DocumentFactsPayload, ExtractedDocumentFacts, FlightItineraryFacts,
    HotelFolioFacts, Location, MoneyAmount, Observed, ReceiptFacts,
};
use crate::draft::{ConfidenceLevel, DraftReport, EvidenceKind, EvidenceReference, FieldMetadata};
use crate::validator::{validate_draft_report, ValidationReport};
use crate::value::ReportValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleIssueSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BundleIssueKind {
    MissingPayeeName,
    ConflictingPayeeName,
    MissingTripWindow,
    MissingDestination,
    MissingReportCategory,
    ConflictingDestination,
    UnprojectedDocument,
    MissingUsdConversion,
    MissingTransactionSummaryTotal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleIssue {
    pub severity: BundleIssueSeverity,
    pub kind: BundleIssueKind,
    pub message: String,
    pub document_ids: Vec<String>,
    pub evidence: Vec<EvidenceReference>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TravelRegion {
    Domestic,
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalPayee {
    pub name: Observed<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalTrip {
    pub window: Option<Observed<DateRange>>,
    pub origin: Option<Observed<Location>>,
    pub destination: Option<Observed<Location>>,
    pub region: Option<Observed<TravelRegion>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalExpenseKind {
    Airfare,
    Lodging,
    GenericReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleSourceDocument {
    pub document_id: String,
    pub filename: String,
    pub document_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalAirfareDetails {
    pub travelers_name: Option<Observed<String>>,
    pub ticket_number: Option<Observed<String>>,
    pub ticket_amount: Option<Observed<String>>,
    pub booking_method: Option<Observed<String>>,
    pub airline: Option<Observed<String>>,
    pub class_of_ticket: Option<Observed<String>>,
    pub departure_airport: Option<Observed<String>>,
    pub destination_airport: Option<Observed<String>>,
    pub round_trip: Option<Observed<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalLodgingDetails {
    pub hotel_name: Option<Observed<String>>,
    pub location: Option<Observed<String>>,
    pub check_in_date: Option<Observed<String>>,
    pub check_out_date: Option<Observed<String>>,
    pub number_of_nights: Option<Observed<String>>,
    pub daily_rate: Option<Observed<String>>,
    pub booking_method: Option<Observed<String>>,
    pub is_shared_lodging: Option<Observed<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalExpenseLine {
    pub line_id: String,
    pub kind: CanonicalExpenseKind,
    pub document_id: String,
    pub date: Option<Observed<String>>,
    pub line_amount_usd: Option<Observed<String>>,
    pub original_currency: Option<Observed<String>>,
    pub original_amount: Option<Observed<String>>,
    pub expense_type: Option<Observed<String>>,
    pub remarks: Option<Observed<String>>,
    pub country_of_activity: Option<Observed<String>>,
    pub foreign_activity_type: Option<Observed<String>>,
    pub source_documents: Vec<BundleSourceDocument>,
    pub airfare_details: Option<CanonicalAirfareDetails>,
    pub lodging_details: Option<CanonicalLodgingDetails>,
    pub projection_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalExpenseBundle {
    pub documents: Vec<ExtractedDocumentFacts>,
    pub payee: Option<CanonicalPayee>,
    pub trip: CanonicalTrip,
    pub expense_lines: Vec<CanonicalExpenseLine>,
    pub issues: Vec<BundleIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleProjectionResult {
    pub bundle: CanonicalExpenseBundle,
    pub draft: DraftReport,
    pub issues: Vec<BundleIssue>,
    pub validation: ValidationReport,
}

#[derive(Debug)]
pub enum RenderCanonicalBundleError {
    Json(serde_json::Error),
}

impl fmt::Display for RenderCanonicalBundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "JSON render error: {err}"),
        }
    }
}

impl std::error::Error for RenderCanonicalBundleError {}

impl From<serde_json::Error> for RenderCanonicalBundleError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn synthesize_bundle(documents: &[ExtractedDocumentFacts]) -> CanonicalExpenseBundle {
    let mut issues = Vec::new();
    let payee = synthesize_payee(documents, &mut issues);
    let window = synthesize_trip_window(documents, &mut issues);
    let origin = synthesize_origin(documents);
    let destination = synthesize_destination(documents, &mut issues);
    let region = synthesize_region(documents, destination.as_ref(), &mut issues);
    let trip = CanonicalTrip {
        window,
        origin,
        destination,
        region,
    };
    let expense_lines = synthesize_expense_lines(documents, &trip, &mut issues);

    CanonicalExpenseBundle {
        documents: documents.to_vec(),
        payee,
        trip,
        expense_lines,
        issues,
    }
}

pub fn project_bundle_to_draft(bundle: &CanonicalExpenseBundle) -> (DraftReport, Vec<BundleIssue>) {
    let mut metadata = BTreeMap::new();
    let mut projection_issues = Vec::new();

    let mut general_information = BTreeMap::new();
    let category = bundle
        .trip
        .region
        .as_ref()
        .map(category_from_region);
    if let Some(category) = category.as_ref() {
        insert_observed_leaf(
            &mut general_information,
            &mut metadata,
            "expense_report.general_information.category",
            "category",
            category,
            |value| ReportValue::String(value.clone()),
        );
    } else {
        projection_issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::MissingReportCategory,
            "Bundle could not infer domestic vs. foreign category".to_owned(),
            bundle
                .documents
                .iter()
                .map(|document| document.document_id.clone())
                .collect(),
            Vec::new(),
        ));
    }

    let mut payee_fields = BTreeMap::new();
    if let Some(payee) = bundle.payee.as_ref() {
        insert_observed_leaf(
            &mut payee_fields,
            &mut metadata,
            "expense_report.general_information.payee.name",
            "name",
            &payee.name,
            |value| ReportValue::String(value.clone()),
        );
    }
    if !payee_fields.is_empty() {
        general_information.insert("payee".to_owned(), ReportValue::Object(payee_fields));
    }

    let rush_processing = default_string_observed(
        "no".to_owned(),
        ConfidenceLevel::High,
        "bundle_synthesis.default_rush_processing",
    );
    insert_observed_leaf(
        &mut general_information,
        &mut metadata,
        "expense_report.general_information.rush_processing",
        "rush_processing",
        &rush_processing,
        |value| ReportValue::String(value.clone()),
    );

    let payment_method = default_string_observed(
        "electronic".to_owned(),
        ConfidenceLevel::High,
        "bundle_synthesis.default_payment_method",
    );
    insert_observed_leaf(
        &mut general_information,
        &mut metadata,
        "expense_report.general_information.payment_method",
        "payment_method",
        &payment_method,
        |value| ReportValue::String(value.clone()),
    );

    let mut business_purpose = BTreeMap::new();
    if let Some(payee) = bundle.payee.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.who",
            "who",
            &payee.name,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(window) = bundle.trip.window.as_ref() {
        let when = system_observed(
            format!("{} to {}", window.value.start_date, window.value.end_date),
            window.confidence,
            window.evidence.clone(),
            "bundle_synthesis.project_business_purpose_when",
            window.flags.clone(),
        );
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.when",
            "when",
            &when,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(destination) = bundle.trip.destination.as_ref() {
        let where_value = system_observed(
            location_to_display(&destination.value),
            destination.confidence,
            destination.evidence.clone(),
            "bundle_synthesis.project_business_purpose_where",
            destination.flags.clone(),
        );
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.where",
            "where",
            &where_value,
            |value| ReportValue::String(value.clone()),
        );
    }

    if !business_purpose.is_empty() {
        general_information.insert(
            "business_purpose".to_owned(),
            ReportValue::Object(business_purpose),
        );
    }

    general_information.insert(
        "student_certification".to_owned(),
        ReportValue::empty_object(),
    );

    let mut transaction_summary = BTreeMap::new();
    if let Some(category) = category.as_ref() {
        let transaction_type = system_observed(
            if category.value == "expenses_foreign" {
                "foreign".to_owned()
            } else {
                "domestic".to_owned()
            },
            category.confidence,
            category.evidence.clone(),
            "bundle_synthesis.project_transaction_type",
            category.flags.clone(),
        );
        insert_observed_leaf(
            &mut transaction_summary,
            &mut metadata,
            "expense_report.transaction_summary.transaction_type",
            "transaction_type",
            &transaction_type,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(transaction_date) = synthesize_transaction_date(bundle) {
        insert_observed_leaf(
            &mut transaction_summary,
            &mut metadata,
            "expense_report.transaction_summary.transaction_date",
            "transaction_date",
            &transaction_date,
            |value| ReportValue::Date(value.clone()),
        );
    }

    if let Some(total_usd) = synthesize_total_usd(bundle) {
        insert_observed_leaf(
            &mut transaction_summary,
            &mut metadata,
            "expense_report.transaction_summary.total_usd",
            "total_usd",
            &total_usd,
            |value| ReportValue::Number(value.clone()),
        );
    } else if bundle
        .expense_lines
        .iter()
        .any(|line| line.projection_supported && line.line_amount_usd.is_none())
    {
        projection_issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::MissingTransactionSummaryTotal,
            "Bundle cannot compute total_usd until every projected line has a USD amount".to_owned(),
            bundle
                .expense_lines
                .iter()
                .filter(|line| line.projection_supported && line.line_amount_usd.is_none())
                .map(|line| line.document_id.clone())
                .collect(),
            Vec::new(),
        ));
    }

    let transaction_lines =
        project_transaction_lines(bundle, &mut metadata, &mut projection_issues);

    let mut report = BTreeMap::new();
    if !general_information.is_empty() {
        report.insert(
            "general_information".to_owned(),
            ReportValue::Object(general_information),
        );
    }
    if !transaction_summary.is_empty() {
        report.insert(
            "transaction_summary".to_owned(),
            ReportValue::Object(transaction_summary),
        );
    }
    if !transaction_lines.is_empty() {
        report.insert(
            "transaction_lines".to_owned(),
            ReportValue::Array(transaction_lines),
        );
    }

    let draft = DraftReport {
        report: ReportValue::Object(report),
        metadata,
    };
    (draft, projection_issues)
}

pub fn synthesize_bundle_projection(documents: &[ExtractedDocumentFacts]) -> BundleProjectionResult {
    let bundle = synthesize_bundle(documents);
    let (draft, projection_issues) = project_bundle_to_draft(&bundle);
    let mut issues = bundle.issues.clone();
    issues.extend(projection_issues);
    let validation = validate_draft_report(&draft);
    BundleProjectionResult {
        bundle,
        draft,
        issues,
        validation,
    }
}

pub fn render_canonical_bundle_json_pretty(
    bundle: &CanonicalExpenseBundle,
) -> Result<String, RenderCanonicalBundleError> {
    Ok(serde_json::to_string_pretty(bundle)?)
}

fn synthesize_payee(
    documents: &[ExtractedDocumentFacts],
    issues: &mut Vec<BundleIssue>,
) -> Option<CanonicalPayee> {
    let mut candidates = Vec::new();

    for document in documents {
        match &document.facts {
            DocumentFactsPayload::FlightItinerary(FlightItineraryFacts { traveler_names, .. }) => {
                if let Some(name) = traveler_names.first() {
                    candidates.push((name.clone(), document.document_id.clone()));
                }
            }
            DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                guest_name: Some(name),
                ..
            }) => candidates.push((name.clone(), document.document_id.clone())),
            DocumentFactsPayload::ConferenceRegistration(facts) => {
                if let Some(name) = facts.attendee_name.as_ref() {
                    candidates.push((name.clone(), document.document_id.clone()));
                }
            }
            _ => {}
        }
    }

    let Some((chosen, chosen_document_id)) = candidates.first().cloned() else {
        issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::MissingPayeeName,
            "Bundle could not infer the payee name from any uploaded document".to_owned(),
            documents
                .iter()
                .map(|document| document.document_id.clone())
                .collect(),
            Vec::new(),
        ));
        return None;
    };

    let chosen_key = normalize_text(&chosen.value);
    let mut conflicting_documents = vec![chosen_document_id];
    let mut conflicting_evidence = chosen.evidence.clone();
    let mut has_conflict = false;

    for (candidate, document_id) in candidates.iter().skip(1) {
        if normalize_text(&candidate.value) != chosen_key {
            has_conflict = true;
            conflicting_documents.push(document_id.clone());
            conflicting_evidence.extend(candidate.evidence.clone());
        }
    }

    if has_conflict {
        issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::ConflictingPayeeName,
            format!(
                "Bundle found conflicting payee names and selected {:?}",
                chosen.value
            ),
            conflicting_documents,
            conflicting_evidence,
        ));
    }

    Some(CanonicalPayee { name: chosen })
}

fn synthesize_trip_window(
    documents: &[ExtractedDocumentFacts],
    issues: &mut Vec<BundleIssue>,
) -> Option<Observed<DateRange>> {
    let mut flight_windows = Vec::new();
    let mut fallback_windows = Vec::new();

    for document in documents {
        match &document.facts {
            DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                trip_window: Some(window),
                ..
            }) => flight_windows.push((window.clone(), document.document_id.clone())),
            DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                stay_window: Some(window),
                ..
            }) => fallback_windows.push((window.clone(), document.document_id.clone())),
            _ => {}
        }
    }

    let candidates = if !flight_windows.is_empty() {
        flight_windows
    } else {
        fallback_windows
    };

    let Some((chosen, _)) = candidates.first().cloned() else {
        issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::MissingTripWindow,
            "Bundle could not infer a trip window".to_owned(),
            documents
                .iter()
                .map(|document| document.document_id.clone())
                .collect(),
            Vec::new(),
        ));
        return None;
    };

    Some(chosen)
}

fn synthesize_origin(documents: &[ExtractedDocumentFacts]) -> Option<Observed<Location>> {
    documents.iter().find_map(|document| match &document.facts {
        DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
            departure_location: Some(location),
            ..
        }) => Some(location.clone()),
        _ => None,
    })
}

fn synthesize_destination(
    documents: &[ExtractedDocumentFacts],
    issues: &mut Vec<BundleIssue>,
) -> Option<Observed<Location>> {
    let mut candidates = Vec::new();

    for document in documents {
        match &document.facts {
            DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                arrival_location: Some(location),
                ..
            }) => candidates.push((location.clone(), document.document_id.clone())),
            DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                property_location: Some(location),
                ..
            }) => candidates.push((location.clone(), document.document_id.clone())),
            DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_location: Some(location),
                ..
            }) => candidates.push((location.clone(), document.document_id.clone())),
            _ => {}
        }
    }

    let Some((chosen, chosen_document_id)) = candidates
        .iter()
        .find(|(candidate, _)| candidate.value.country.is_some())
        .cloned()
        .or_else(|| candidates.first().cloned())
    else {
        issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::MissingDestination,
            "Bundle could not infer a primary destination".to_owned(),
            documents
                .iter()
                .map(|document| document.document_id.clone())
                .collect(),
            Vec::new(),
        ));
        return None;
    };

    let mut conflicting_documents = vec![chosen_document_id];
    let mut conflicting_evidence = chosen.evidence.clone();
    let mut has_conflict = false;

    for (candidate, document_id) in &candidates {
        if !same_location(&chosen.value, &candidate.value) {
            has_conflict = true;
            conflicting_documents.push(document_id.clone());
            conflicting_evidence.extend(candidate.evidence.clone());
        }
    }

    if has_conflict {
        issues.push(bundle_issue(
            BundleIssueSeverity::Warning,
            BundleIssueKind::ConflictingDestination,
            format!(
                "Bundle found conflicting destination candidates and selected {}",
                location_to_display(&chosen.value)
            ),
            conflicting_documents,
            conflicting_evidence,
        ));
    }

    Some(chosen)
}

fn synthesize_region(
    documents: &[ExtractedDocumentFacts],
    destination: Option<&Observed<Location>>,
    issues: &mut Vec<BundleIssue>,
) -> Option<Observed<TravelRegion>> {
    if let Some(destination) = destination {
        if let Some(country) = destination.value.country.as_deref() {
            return Some(system_observed(
                if is_us_country(country) {
                    TravelRegion::Domestic
                } else {
                    TravelRegion::Foreign
                },
                destination.confidence,
                destination.evidence.clone(),
                "bundle_synthesis.infer_region_from_destination_country",
                destination.flags.clone(),
            ));
        }
    }

    let mut has_non_usd = false;
    let mut has_any_currency = false;
    let mut evidence = Vec::new();
    for document in documents {
        let amount = match &document.facts {
            DocumentFactsPayload::FlightItinerary(facts) => facts.total_paid.as_ref(),
            DocumentFactsPayload::HotelFolio(facts) => facts.total_paid.as_ref(),
            DocumentFactsPayload::Receipt(facts) => facts.total_paid.as_ref(),
            _ => None,
        };

        if let Some(amount) = amount {
            if let Some(currency) = amount.value.currency.as_deref() {
                has_any_currency = true;
                evidence.extend(amount.evidence.clone());
                if !currency.eq_ignore_ascii_case("USD") {
                    has_non_usd = true;
                }
            }
        }
    }

    if has_non_usd {
        return Some(system_observed(
            TravelRegion::Foreign,
            ConfidenceLevel::Medium,
            evidence,
            "bundle_synthesis.infer_region_from_non_usd_currency",
            Vec::new(),
        ));
    }

    if has_any_currency {
        return Some(system_observed(
            TravelRegion::Domestic,
            ConfidenceLevel::Low,
            evidence,
            "bundle_synthesis.infer_region_from_usd_currency_only",
            vec!["currency_only_inference".to_owned()],
        ));
    }

    issues.push(bundle_issue(
        BundleIssueSeverity::Warning,
        BundleIssueKind::MissingReportCategory,
        "Bundle could not infer a report category from destination or currency context".to_owned(),
        documents
            .iter()
            .map(|document| document.document_id.clone())
            .collect(),
        Vec::new(),
    ));
    None
}

fn synthesize_expense_lines(
    documents: &[ExtractedDocumentFacts],
    trip: &CanonicalTrip,
    issues: &mut Vec<BundleIssue>,
) -> Vec<CanonicalExpenseLine> {
    let mut lines = Vec::new();

    for document in documents {
        match &document.facts {
            DocumentFactsPayload::FlightItinerary(facts) => {
                lines.push(synthesize_airfare_line(document, facts, trip));
            }
            DocumentFactsPayload::HotelFolio(facts) => {
                let line = synthesize_lodging_line(document, facts, trip);
                if line.line_amount_usd.is_none()
                    && matches!(
                        trip.region.as_ref().map(|region| region.value),
                        Some(TravelRegion::Foreign)
                    )
                {
                    issues.push(bundle_issue(
                        BundleIssueSeverity::Warning,
                        BundleIssueKind::MissingUsdConversion,
                        format!(
                            "Document {} needs FX enrichment before it can produce a complete lodging line",
                            document.filename
                        ),
                        vec![document.document_id.clone()],
                        line
                            .original_amount
                            .as_ref()
                            .map(|amount| amount.evidence.clone())
                            .unwrap_or_default(),
                    ));
                }
                lines.push(line);
            }
            DocumentFactsPayload::Receipt(facts) => {
                let line = synthesize_generic_receipt_line(document, facts);
                issues.push(bundle_issue(
                    BundleIssueSeverity::Warning,
                    BundleIssueKind::UnprojectedDocument,
                    format!(
                        "Document {} remains a generic receipt and is not yet projected into schema transaction lines",
                        document.filename
                    ),
                    vec![document.document_id.clone()],
                    line
                        .original_amount
                        .as_ref()
                        .map(|amount| amount.evidence.clone())
                        .unwrap_or_default(),
                ));
                lines.push(line);
            }
            payload => {
                issues.push(bundle_issue(
                    BundleIssueSeverity::Warning,
                    BundleIssueKind::UnprojectedDocument,
                    format!(
                        "Document kind {} is not yet supported by bundle projection",
                        payload.kind().as_str()
                    ),
                    vec![document.document_id.clone()],
                    document.classification.evidence.clone(),
                ));
            }
        }
    }

    lines
}

fn synthesize_airfare_line(
    document: &ExtractedDocumentFacts,
    facts: &FlightItineraryFacts,
    trip: &CanonicalTrip,
) -> CanonicalExpenseLine {
    let amount = facts.total_paid.as_ref();
    let date = facts
        .booking_date
        .as_ref()
        .map(|date| clone_string_observed(date))
        .or_else(|| {
            facts.segments.first().map(|segment| {
                system_observed(
                    segment.departure_date.value.clone(),
                    ConfidenceLevel::Medium,
                    segment.departure_date.evidence.clone(),
                    "bundle_synthesis.infer_airfare_date_from_departure",
                    Vec::new(),
                )
            })
        });
    let line_amount_usd = amount.and_then(|amount| usd_amount_from_money(amount));
    let original_currency = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| amount.and_then(currency_observed_from_money));
    let original_amount = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| amount.map(number_observed_from_money));
    let expense_type = trip.region.as_ref().map(|region| {
        system_observed(
            match region.value {
                TravelRegion::Domestic => "airfare_domestic".to_owned(),
                TravelRegion::Foreign => "airfare_foreign".to_owned(),
            },
            ConfidenceLevel::High,
            region.evidence.clone(),
            "bundle_synthesis.classify_airfare_expense_type",
            Vec::new(),
        )
    });

    let remarks = Some(system_observed(
        format!(
            "Airfare {} to {}",
            facts.segments
                .first()
                .map(|segment| segment.departure_airport.value.clone())
                .unwrap_or_else(|| "unknown origin".to_owned()),
            facts.segments
                .last()
                .map(|segment| segment.arrival_airport.value.clone())
                .unwrap_or_else(|| "unknown destination".to_owned())
        ),
        ConfidenceLevel::Medium,
        amount.map(|value| value.evidence.clone()).unwrap_or_default(),
        "bundle_synthesis.build_airfare_remarks",
        Vec::new(),
    ));

    let country_of_activity = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| {
            trip.destination.as_ref().and_then(|destination| {
                destination.value.country.as_ref().map(|country| {
                    system_observed(
                        country.clone(),
                        destination.confidence,
                        destination.evidence.clone(),
                        "bundle_synthesis.project_country_of_activity",
                        destination.flags.clone(),
                    )
                })
            })
        });
    let foreign_activity_type = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .map(|region| {
            system_observed(
                "other".to_owned(),
                ConfidenceLevel::Low,
                region.evidence.clone(),
                "bundle_synthesis.default_foreign_activity_type",
                vec!["requires_activity_review".to_owned()],
            )
        });

    let first_segment = facts.segments.first();
    let last_segment = facts.segments.last();
    let airfare_details = Some(CanonicalAirfareDetails {
        travelers_name: facts
            .traveler_names
            .first()
            .map(|value| clone_string_observed(value)),
        ticket_number: facts.ticket_number.as_ref().map(clone_string_observed),
        ticket_amount: amount.map(number_observed_from_money),
        booking_method: Some(default_string_observed(
            "other".to_owned(),
            ConfidenceLevel::High,
            "bundle_synthesis.default_airfare_booking_method",
        )),
        airline: first_segment.and_then(|segment| {
            segment
                .marketing_carrier
                .as_ref()
                .map(clone_string_observed)
        }),
        class_of_ticket: first_segment.and_then(|segment| {
            segment.cabin_class.as_ref().map(|cabin| {
                system_observed(
                    normalize_airfare_class(&cabin.value),
                    cabin.confidence,
                    cabin.evidence.clone(),
                    "bundle_synthesis.normalize_airfare_class",
                    cabin.flags.clone(),
                )
            })
        }),
        departure_airport: first_segment.map(|segment| clone_string_observed(&segment.departure_airport)),
        destination_airport: last_segment.map(|segment| clone_string_observed(&segment.arrival_airport)),
        round_trip: first_segment.zip(last_segment).map(|(first, last)| {
            let is_round_trip = first.departure_airport.value == last.arrival_airport.value;
            let mut evidence = first.departure_airport.evidence.clone();
            evidence.extend(last.arrival_airport.evidence.clone());
            system_observed(
                is_round_trip,
                ConfidenceLevel::Medium,
                evidence,
                "bundle_synthesis.detect_round_trip",
                Vec::new(),
            )
        }),
    });

    CanonicalExpenseLine {
        line_id: format!("{}::airfare", document.document_id),
        kind: CanonicalExpenseKind::Airfare,
        document_id: document.document_id.clone(),
        date,
        line_amount_usd,
        original_currency,
        original_amount,
        expense_type,
        remarks,
        country_of_activity,
        foreign_activity_type,
        source_documents: vec![BundleSourceDocument {
            document_id: document.document_id.clone(),
            filename: document.filename.clone(),
            document_type: "booking_confirmation".to_owned(),
        }],
        airfare_details,
        lodging_details: None,
        projection_supported: true,
    }
}

fn synthesize_lodging_line(
    document: &ExtractedDocumentFacts,
    facts: &HotelFolioFacts,
    trip: &CanonicalTrip,
) -> CanonicalExpenseLine {
    let amount = facts.total_paid.as_ref();
    let date = facts
        .stay_window
        .as_ref()
        .map(|window| {
            system_observed(
                window.value.end_date.clone(),
                window.confidence,
                window.evidence.clone(),
                "bundle_synthesis.project_lodging_line_date_from_checkout",
                window.flags.clone(),
            )
        })
        .or_else(|| facts.nightly_charges.last().map(|night| clone_string_observed(&night.date)));
    let line_amount_usd = amount.and_then(|amount| usd_amount_from_money(amount));
    let original_currency = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| amount.and_then(currency_observed_from_money));
    let original_amount = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| amount.map(number_observed_from_money));
    let expense_type = trip.region.as_ref().map(|region| {
        system_observed(
            match region.value {
                TravelRegion::Domestic => "lodging_domestic".to_owned(),
                TravelRegion::Foreign => "lodging_foreign".to_owned(),
            },
            ConfidenceLevel::High,
            region.evidence.clone(),
            "bundle_synthesis.classify_lodging_expense_type",
            Vec::new(),
        )
    });
    let remarks = Some(system_observed(
        format!(
            "Lodging at {}",
            facts.property_name
                .as_ref()
                .map(|value| value.value.clone())
                .unwrap_or_else(|| "unknown hotel".to_owned())
        ),
        ConfidenceLevel::Medium,
        amount.map(|value| value.evidence.clone()).unwrap_or_default(),
        "bundle_synthesis.build_lodging_remarks",
        Vec::new(),
    ));
    let country_of_activity = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| {
            facts.property_location.as_ref().and_then(|location| {
                location.value.country.as_ref().map(|country| {
                    system_observed(
                        country.clone(),
                        location.confidence,
                        location.evidence.clone(),
                        "bundle_synthesis.project_country_of_activity",
                        location.flags.clone(),
                    )
                })
            })
        });
    let foreign_activity_type = trip
        .region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .map(|region| {
            system_observed(
                "other".to_owned(),
                ConfidenceLevel::Low,
                region.evidence.clone(),
                "bundle_synthesis.default_foreign_activity_type",
                vec!["requires_activity_review".to_owned()],
            )
        });

    let daily_rate = synthesize_daily_rate(facts);
    let lodging_details = Some(CanonicalLodgingDetails {
        hotel_name: facts.property_name.as_ref().map(clone_string_observed),
        location: facts.property_location.as_ref().map(|location| {
            system_observed(
                location_to_display(&location.value),
                location.confidence,
                location.evidence.clone(),
                "bundle_synthesis.project_lodging_location",
                location.flags.clone(),
            )
        }),
        check_in_date: facts.stay_window.as_ref().map(|window| {
            system_observed(
                window.value.start_date.clone(),
                window.confidence,
                window.evidence.clone(),
                "bundle_synthesis.project_lodging_check_in",
                window.flags.clone(),
            )
        }),
        check_out_date: facts.stay_window.as_ref().map(|window| {
            system_observed(
                window.value.end_date.clone(),
                window.confidence,
                window.evidence.clone(),
                "bundle_synthesis.project_lodging_check_out",
                window.flags.clone(),
            )
        }),
        number_of_nights: if facts.nightly_charges.is_empty() {
            None
        } else {
            Some(system_observed(
                facts.nightly_charges.len().to_string(),
                ConfidenceLevel::High,
                facts.nightly_charges
                    .iter()
                    .flat_map(|night| night.date.evidence.clone())
                    .collect(),
                "bundle_synthesis.compute_number_of_nights",
                Vec::new(),
            ))
        },
        daily_rate,
        booking_method: Some(default_string_observed(
            "other".to_owned(),
            ConfidenceLevel::High,
            "bundle_synthesis.default_lodging_booking_method",
        )),
        is_shared_lodging: Some(system_observed(
            false,
            ConfidenceLevel::Medium,
            facts
                .property_name
                .as_ref()
                .map(|value| value.evidence.clone())
                .unwrap_or_default(),
            "bundle_synthesis.default_shared_lodging_false",
            Vec::new(),
        )),
    });

    CanonicalExpenseLine {
        line_id: format!("{}::lodging", document.document_id),
        kind: CanonicalExpenseKind::Lodging,
        document_id: document.document_id.clone(),
        date,
        line_amount_usd,
        original_currency,
        original_amount,
        expense_type,
        remarks,
        country_of_activity,
        foreign_activity_type,
        source_documents: vec![BundleSourceDocument {
            document_id: document.document_id.clone(),
            filename: document.filename.clone(),
            document_type: "receipt".to_owned(),
        }],
        airfare_details: None,
        lodging_details,
        projection_supported: true,
    }
}

fn synthesize_generic_receipt_line(
    document: &ExtractedDocumentFacts,
    facts: &ReceiptFacts,
) -> CanonicalExpenseLine {
    CanonicalExpenseLine {
        line_id: format!("{}::receipt", document.document_id),
        kind: CanonicalExpenseKind::GenericReceipt,
        document_id: document.document_id.clone(),
        date: facts.transaction_date.as_ref().map(clone_string_observed),
        line_amount_usd: facts.total_paid.as_ref().and_then(usd_amount_from_money),
        original_currency: facts.total_paid.as_ref().and_then(currency_observed_from_money),
        original_amount: facts.total_paid.as_ref().map(number_observed_from_money),
        expense_type: None,
        remarks: facts.merchant_name.as_ref().map(|merchant| {
            system_observed(
                format!("Receipt from {}", merchant.value),
                merchant.confidence,
                merchant.evidence.clone(),
                "bundle_synthesis.generic_receipt_remarks",
                merchant.flags.clone(),
            )
        }),
        country_of_activity: facts.merchant_location.as_ref().and_then(|location| {
            location.value.country.as_ref().map(|country| {
                system_observed(
                    country.clone(),
                    location.confidence,
                    location.evidence.clone(),
                    "bundle_synthesis.project_country_of_activity",
                    location.flags.clone(),
                )
            })
        }),
        foreign_activity_type: None,
        source_documents: vec![BundleSourceDocument {
            document_id: document.document_id.clone(),
            filename: document.filename.clone(),
            document_type: "receipt".to_owned(),
        }],
        airfare_details: None,
        lodging_details: None,
        projection_supported: false,
    }
}

fn project_transaction_lines(
    bundle: &CanonicalExpenseBundle,
    metadata: &mut BTreeMap<String, FieldMetadata>,
    issues: &mut Vec<BundleIssue>,
) -> Vec<ReportValue> {
    let mut lines = Vec::new();

    for line in bundle.expense_lines.iter().filter(|line| line.projection_supported) {
        let line_index = lines.len();
        let base_path = format!("expense_report.transaction_lines[{line_index}]");

        let mut line_object = BTreeMap::new();
        let mut common = BTreeMap::new();

        if let Some(date) = line.date.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.date"),
                "date",
                date,
                |value| ReportValue::Date(value.clone()),
            );
        }
        if let Some(line_amount_usd) = line.line_amount_usd.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.line_amount_usd"),
                "line_amount_usd",
                line_amount_usd,
                |value| ReportValue::Number(value.clone()),
            );
        }
        if let Some(original_currency) = line.original_currency.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.original_currency"),
                "original_currency",
                original_currency,
                |value| ReportValue::String(value.clone()),
            );
        }
        if let Some(original_amount) = line.original_amount.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.original_amount"),
                "original_amount",
                original_amount,
                |value| ReportValue::Number(value.clone()),
            );
        }
        if let Some(expense_type) = line.expense_type.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.expense_type"),
                "expense_type",
                expense_type,
                |value| ReportValue::String(value.clone()),
            );
        }
        if let Some(remarks) = line.remarks.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.remarks"),
                "remarks",
                remarks,
                |value| ReportValue::String(value.clone()),
            );
        }
        if let Some(country_of_activity) = line.country_of_activity.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.country_of_activity"),
                "country_of_activity",
                country_of_activity,
                |value| ReportValue::String(value.clone()),
            );
        }
        if let Some(foreign_activity_type) = line.foreign_activity_type.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.foreign_activity_type"),
                "foreign_activity_type",
                foreign_activity_type,
                |value| ReportValue::String(value.clone()),
            );
        }
        if !line.source_documents.is_empty() {
            let source_documents = line
                .source_documents
                .iter()
                .enumerate()
                .map(|(source_index, source_document)| {
                    let source_path =
                        format!("{base_path}.common.source_documents[{source_index}]");
                    let mut source_object = BTreeMap::new();

                    let filename = default_observed_with_document(
                        source_document.filename.clone(),
                        ConfidenceLevel::High,
                        &source_document.document_id,
                        &source_document.filename,
                    );
                    insert_observed_leaf(
                        &mut source_object,
                        metadata,
                        &format!("{source_path}.filename"),
                        "filename",
                        &filename,
                        |value| ReportValue::String(value.clone()),
                    );

                    let document_type = system_observed(
                        source_document.document_type.clone(),
                        ConfidenceLevel::High,
                        vec![document_reference(
                            &source_document.document_id,
                            &source_document.filename,
                        )],
                        "bundle_synthesis.map_source_document_type",
                        Vec::new(),
                    );
                    insert_observed_leaf(
                        &mut source_object,
                        metadata,
                        &format!("{source_path}.document_type"),
                        "document_type",
                        &document_type,
                        |value| ReportValue::String(value.clone()),
                    );

                    ReportValue::Object(source_object)
                })
                .collect::<Vec<_>>();
            common.insert(
                "source_documents".to_owned(),
                ReportValue::Array(source_documents),
            );
        }
        if !common.is_empty() {
            line_object.insert("common".to_owned(), ReportValue::Object(common));
        }

        match line.kind {
            CanonicalExpenseKind::Airfare => {
                let mut airfare_details = BTreeMap::new();
                if let Some(details) = line.airfare_details.as_ref() {
                    if let Some(travelers_name) = details.travelers_name.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.travelers_name"),
                            "travelers_name",
                            travelers_name,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(ticket_number) = details.ticket_number.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.ticket_number"),
                            "ticket_number",
                            ticket_number,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(ticket_amount) = details.ticket_amount.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.ticket_amount"),
                            "ticket_amount",
                            ticket_amount,
                            |value| ReportValue::Number(value.clone()),
                        );
                    }
                    if let Some(booking_method) = details.booking_method.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.booking_method"),
                            "booking_method",
                            booking_method,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(airline) = details.airline.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.airline"),
                            "airline",
                            airline,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(class_of_ticket) = details.class_of_ticket.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.class_of_ticket"),
                            "class_of_ticket",
                            class_of_ticket,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(departure_airport) = details.departure_airport.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.departure_airport"),
                            "departure_airport",
                            departure_airport,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(destination_airport) = details.destination_airport.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.destination_airport"),
                            "destination_airport",
                            destination_airport,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(round_trip) = details.round_trip.as_ref() {
                        insert_observed_leaf(
                            &mut airfare_details,
                            metadata,
                            &format!("{base_path}.airfare_details.round_trip"),
                            "round_trip",
                            round_trip,
                            |value| ReportValue::Bool(*value),
                        );
                    }
                }
                airfare_details.insert(
                    "price_comparison".to_owned(),
                    ReportValue::empty_object(),
                );
                line_object.insert(
                    "airfare_details".to_owned(),
                    ReportValue::Object(airfare_details),
                );
            }
            CanonicalExpenseKind::Lodging => {
                let mut lodging_details = BTreeMap::new();
                if let Some(details) = line.lodging_details.as_ref() {
                    if let Some(hotel_name) = details.hotel_name.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.hotel_name"),
                            "hotel_name",
                            hotel_name,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(location) = details.location.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.location"),
                            "location",
                            location,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(check_in_date) = details.check_in_date.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.check_in_date"),
                            "check_in_date",
                            check_in_date,
                            |value| ReportValue::Date(value.clone()),
                        );
                    }
                    if let Some(check_out_date) = details.check_out_date.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.check_out_date"),
                            "check_out_date",
                            check_out_date,
                            |value| ReportValue::Date(value.clone()),
                        );
                    }
                    if let Some(number_of_nights) = details.number_of_nights.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.number_of_nights"),
                            "number_of_nights",
                            number_of_nights,
                            |value| ReportValue::Number(value.clone()),
                        );
                    }
                    if let Some(daily_rate) = details.daily_rate.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.daily_rate"),
                            "daily_rate",
                            daily_rate,
                            |value| ReportValue::Number(value.clone()),
                        );
                    }
                    if let Some(booking_method) = details.booking_method.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.booking_method"),
                            "booking_method",
                            booking_method,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(is_shared_lodging) = details.is_shared_lodging.as_ref() {
                        insert_observed_leaf(
                            &mut lodging_details,
                            metadata,
                            &format!("{base_path}.lodging_details.is_shared_lodging"),
                            "is_shared_lodging",
                            is_shared_lodging,
                            |value| ReportValue::Bool(*value),
                        );
                    }
                }
                line_object.insert(
                    "lodging_details".to_owned(),
                    ReportValue::Object(lodging_details),
                );
            }
            CanonicalExpenseKind::GenericReceipt => {
                issues.push(bundle_issue(
                    BundleIssueSeverity::Warning,
                    BundleIssueKind::UnprojectedDocument,
                    format!(
                        "Document {} is present in the bundle but not projected into a schema line",
                        line.document_id
                    ),
                    vec![line.document_id.clone()],
                    Vec::new(),
                ));
            }
        }

        lines.push(ReportValue::Object(line_object));
    }

    lines
}

fn synthesize_transaction_date(bundle: &CanonicalExpenseBundle) -> Option<Observed<String>> {
    if let Some(window) = bundle.trip.window.as_ref() {
        return Some(system_observed(
            window.value.end_date.clone(),
            window.confidence,
            window.evidence.clone(),
            "bundle_synthesis.project_transaction_date_from_trip_end",
            window.flags.clone(),
        ));
    }

    bundle
        .expense_lines
        .iter()
        .filter_map(|line| line.date.as_ref())
        .max_by_key(|date| date.value.clone())
        .map(clone_string_observed)
}

fn synthesize_total_usd(bundle: &CanonicalExpenseBundle) -> Option<Observed<String>> {
    let supported_lines = bundle
        .expense_lines
        .iter()
        .filter(|line| line.projection_supported)
        .collect::<Vec<_>>();

    if supported_lines.is_empty() || supported_lines.iter().any(|line| line.line_amount_usd.is_none()) {
        return None;
    }

    let mut evidence = Vec::new();
    let mut total_cents = 0i64;
    for line in supported_lines {
        let amount = line.line_amount_usd.as_ref()?;
        total_cents += amount_to_cents(&amount.value)?;
        evidence.extend(amount.evidence.clone());
    }

    Some(system_observed(
        cents_to_amount(total_cents),
        ConfidenceLevel::High,
        evidence,
        "bundle_synthesis.compute_total_usd",
        Vec::new(),
    ))
}

fn synthesize_daily_rate(facts: &HotelFolioFacts) -> Option<Observed<String>> {
    let room_rates = facts
        .nightly_charges
        .iter()
        .filter_map(|night| night.room_rate.as_ref())
        .collect::<Vec<_>>();
    if room_rates.is_empty() {
        return None;
    }

    let currencies = room_rates
        .iter()
        .filter_map(|rate| rate.value.currency.as_deref())
        .map(|currency| currency.to_ascii_uppercase())
        .collect::<Vec<_>>();
    if !currencies.is_empty() && currencies.iter().any(|currency| currency != &currencies[0]) {
        return None;
    }

    let mut total_cents = 0i64;
    let mut evidence = Vec::new();
    for rate in &room_rates {
        total_cents += amount_to_cents(&rate.value.amount)?;
        evidence.extend(rate.evidence.clone());
    }
    let average_cents = total_cents / i64::try_from(room_rates.len()).ok()?;

    Some(system_observed(
        cents_to_amount(average_cents),
        ConfidenceLevel::High,
        evidence,
        "bundle_synthesis.compute_lodging_daily_rate",
        Vec::new(),
    ))
}

fn category_from_region(region: &Observed<TravelRegion>) -> Observed<String> {
    system_observed(
        match region.value {
            TravelRegion::Domestic => "expenses_domestic".to_owned(),
            TravelRegion::Foreign => "expenses_foreign".to_owned(),
        },
        region.confidence,
        region.evidence.clone(),
        "bundle_synthesis.project_category_from_region",
        region.flags.clone(),
    )
}

fn usd_amount_from_money(value: &Observed<MoneyAmount>) -> Option<Observed<String>> {
    let currency = value.value.currency.as_deref();
    if currency.is_none() || currency == Some("USD") {
        Some(system_observed(
            value.value.amount.clone(),
            value.confidence,
            value.evidence.clone(),
            "bundle_synthesis.project_usd_amount",
            value.flags.clone(),
        ))
    } else {
        None
    }
}

fn currency_observed_from_money(value: &Observed<MoneyAmount>) -> Option<Observed<String>> {
    let currency = value.value.currency.as_ref()?.clone();
    Some(system_observed(
        currency,
        value.confidence,
        value.evidence.clone(),
        "bundle_synthesis.project_original_currency",
        value.flags.clone(),
    ))
}

fn number_observed_from_money(value: &Observed<MoneyAmount>) -> Observed<String> {
    system_observed(
        value.value.amount.clone(),
        value.confidence,
        value.evidence.clone(),
        "bundle_synthesis.project_amount_number",
        value.flags.clone(),
    )
}

fn clone_string_observed(value: &Observed<String>) -> Observed<String> {
    Observed {
        value: value.value.clone(),
        confidence: value.confidence,
        evidence: value.evidence.clone(),
        flags: value.flags.clone(),
    }
}

fn normalize_airfare_class(value: &str) -> String {
    match normalize_text(value).as_str() {
        "economy" | "coach" => "coach".to_owned(),
        "premium economy" | "premium_economy" => "premium_economy".to_owned(),
        "business" => "business".to_owned(),
        "first" => "first".to_owned(),
        _ => "coach".to_owned(),
    }
}

fn default_string_observed(
    value: String,
    confidence: ConfidenceLevel,
    origin: &str,
) -> Observed<String> {
    system_observed(value, confidence, Vec::new(), origin, Vec::new())
}

fn default_observed_with_document(
    value: String,
    confidence: ConfidenceLevel,
    document_id: &str,
    filename: &str,
) -> Observed<String> {
    Observed {
        value,
        confidence,
        evidence: vec![document_reference(document_id, filename)],
        flags: Vec::new(),
    }
}

fn system_observed<T>(
    value: T,
    confidence: ConfidenceLevel,
    mut evidence: Vec<EvidenceReference>,
    origin: &str,
    flags: Vec<String>,
) -> Observed<T> {
    evidence.push(EvidenceReference {
        kind: EvidenceKind::SystemGenerated,
        document_id: None,
        filename: None,
        page: None,
        quote: None,
        origin: Some(origin.to_owned()),
    });

    Observed {
        value,
        confidence,
        evidence,
        flags,
    }
}

fn document_reference(document_id: &str, filename: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::Document,
        document_id: Some(document_id.to_owned()),
        filename: Some(filename.to_owned()),
        page: None,
        quote: None,
        origin: None,
    }
}

fn bundle_issue(
    severity: BundleIssueSeverity,
    kind: BundleIssueKind,
    message: String,
    document_ids: Vec<String>,
    evidence: Vec<EvidenceReference>,
) -> BundleIssue {
    BundleIssue {
        severity,
        kind,
        message,
        document_ids,
        evidence,
    }
}

fn insert_observed_leaf<T>(
    parent: &mut BTreeMap<String, ReportValue>,
    metadata: &mut BTreeMap<String, FieldMetadata>,
    path: &str,
    key: &str,
    observed: &Observed<T>,
    to_value: impl FnOnce(&T) -> ReportValue,
) {
    parent.insert(key.to_owned(), to_value(&observed.value));
    metadata.insert(path.to_owned(), field_metadata_from_observed(observed));
}

fn field_metadata_from_observed<T>(observed: &Observed<T>) -> FieldMetadata {
    FieldMetadata {
        confidence: observed.confidence,
        evidence: observed.evidence.clone(),
        needs_review: observed.needs_review(),
        flags: observed.flags.clone(),
    }
}

fn location_to_display(location: &Location) -> String {
    let mut parts = Vec::new();
    if let Some(city) = location.city.as_deref() {
        parts.push(city.to_owned());
    }
    if let Some(region) = location.region.as_deref() {
        parts.push(region.to_owned());
    }
    if let Some(country) = location.country.as_deref() {
        if parts.last().map(|part| normalize_text(part)) != Some(normalize_text(country)) {
            parts.push(country.to_owned());
        }
    }

    if parts.is_empty() {
        location
            .airport_code
            .clone()
            .unwrap_or_else(|| "Unknown".to_owned())
    } else {
        parts.join(", ")
    }
}

fn same_location(lhs: &Location, rhs: &Location) -> bool {
    normalize_optional_text(lhs.city.as_deref()) == normalize_optional_text(rhs.city.as_deref())
        && normalize_optional_text(lhs.country.as_deref())
            == normalize_optional_text(rhs.country.as_deref())
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.map(normalize_text)
}

fn normalize_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'A'..='Z' => ch.to_ascii_lowercase(),
            ',' | '.' | '/' | '-' | '_' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_us_country(value: &str) -> bool {
    matches!(
        normalize_text(value).as_str(),
        "united states" | "united states of america" | "usa" | "us"
    )
}

fn amount_to_cents(amount: &str) -> Option<i64> {
    let (whole, fraction) = amount.split_once('.')?;
    let whole = whole.parse::<i64>().ok()?;
    let fraction = format!("{:0<2}", fraction);
    let fraction = fraction.get(0..2)?.parse::<i64>().ok()?;
    Some(whole * 100 + fraction)
}

fn cents_to_amount(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents.abs() % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curated_corpus::curated_corpus_root;
    use crate::document_facts::parse_document_facts_json_path;
    use crate::DocumentKind;
    use crate::synthetic_documents::{generate_synthetic_document, SyntheticVariant};
    use crate::validator::ValidationIssueKind;

    fn synthetic_docs() -> Vec<ExtractedDocumentFacts> {
        vec![
            generate_synthetic_document(DocumentKind::FlightItinerary, SyntheticVariant::Baseline)
                .expected_facts,
            generate_synthetic_document(DocumentKind::HotelFolio, SyntheticVariant::Baseline)
                .expected_facts,
            generate_synthetic_document(DocumentKind::Receipt, SyntheticVariant::Baseline)
                .expected_facts,
        ]
    }

    fn curated_sidecar(name: &str) -> ExtractedDocumentFacts {
        let path = curated_corpus_root().join(name);
        parse_document_facts_json_path(path).expect("curated sidecar should parse")
    }

    fn get_path<'a>(value: &'a ReportValue, path: &str) -> Option<&'a ReportValue> {
        let mut current = value;
        for segment in path.split('.') {
            let mut segment = segment;
            loop {
                if let Some(index_start) = segment.find('[') {
                    let key = &segment[..index_start];
                    if !key.is_empty() {
                        current = current.as_object()?.get(key)?;
                    }
                    let remainder = &segment[index_start + 1..];
                    let index_end = remainder.find(']')?;
                    let index = remainder[..index_end].parse::<usize>().ok()?;
                    current = current.as_array()?.get(index)?;
                    segment = &remainder[index_end + 1..];
                    if segment.is_empty() {
                        break;
                    }
                } else {
                    current = current.as_object()?.get(segment)?;
                    break;
                }
            }
        }
        Some(current)
    }

    #[test]
    fn synthesizes_consistent_bundle_from_synthetic_documents() {
        let bundle = synthesize_bundle(&synthetic_docs());

        assert_eq!(bundle.payee.as_ref().map(|payee| payee.name.value.as_str()), Some("Olivia Park"));
        assert_eq!(
            bundle.trip.destination.as_ref().and_then(|destination| destination.value.country.as_deref()),
            Some("Singapore")
        );
        assert_eq!(
            bundle.trip.region.as_ref().map(|region| region.value),
            Some(TravelRegion::Foreign)
        );
        assert_eq!(bundle.expense_lines.len(), 3);
        assert!(bundle
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::UnprojectedDocument));
    }

    #[test]
    fn flags_conflicting_payee_names() {
        let mut documents = synthetic_docs();
        let DocumentFactsPayload::HotelFolio(facts) = &mut documents[1].facts else {
            panic!("expected hotel facts");
        };
        facts.guest_name.as_mut().unwrap().value = "Olive Park".to_owned();

        let bundle = synthesize_bundle(&documents);
        assert!(bundle
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::ConflictingPayeeName));
    }

    #[test]
    fn projects_bundle_into_partial_draft() {
        let result = synthesize_bundle_projection(&synthetic_docs());

        assert_eq!(
            get_path(&result.draft.report, "general_information.category").and_then(ReportValue::as_text),
            Some("expenses_foreign")
        );
        assert_eq!(
            get_path(&result.draft.report, "general_information.rush_processing").and_then(ReportValue::as_text),
            Some("no")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.transaction_type").and_then(ReportValue::as_text),
            Some("foreign")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[0].common.expense_type")
                .and_then(ReportValue::as_text),
            Some("airfare_foreign")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[1].common.expense_type")
                .and_then(ReportValue::as_text),
            Some("lodging_foreign")
        );
        assert!(result
            .draft
            .metadata
            .contains_key("expense_report.transaction_lines[0].common.expense_type"));
    }

    #[test]
    fn projected_draft_surfaces_expected_validation_gaps() {
        let result = synthesize_bundle_projection(&synthetic_docs());

        assert!(result.validation.has_errors());
        assert!(result
            .validation
            .issues
            .iter()
            .any(|issue| issue.kind == ValidationIssueKind::MissingRequiredField
                && issue.path == "expense_report.general_information.payee.affiliation"));
        assert!(result
            .validation
            .issues
            .iter()
            .any(|issue| issue.kind == ValidationIssueKind::MissingRequiredField
                && issue.path == "expense_report.transaction_summary.total_usd"));
        assert!(result
            .validation
            .issues
            .iter()
            .any(|issue| issue.kind == ValidationIssueKind::MissingRequiredField
                && issue.path == "expense_report.transaction_lines[1].common.line_amount_usd"));
    }

    #[test]
    fn projected_foreign_airfare_uses_identity_usd_exchange_context() {
        let result = synthesize_bundle_projection(&synthetic_docs());

        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[0].common.original_currency")
                .and_then(ReportValue::as_text),
            Some("USD")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[0].common.original_amount")
                .and_then(ReportValue::as_text),
            Some("1287.44")
        );
    }

    #[test]
    fn synthesizes_bundle_from_curated_sidecars() {
        let documents = vec![
            curated_sidecar("flight_itinerary/airline_itinerary_classic.md.expected.json"),
            curated_sidecar("hotel_folio/hotel_folio_guest_bill.md.expected.json"),
            curated_sidecar("receipt/receipt_card_dotted.md.expected.json"),
        ];

        let result = synthesize_bundle_projection(&documents);

        assert_eq!(
            result.bundle.payee.as_ref().map(|payee| payee.name.value.as_str()),
            Some("Olivia Park")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[0].airfare_details.airline")
                .and_then(ReportValue::as_text),
            Some("ANA")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_lines[1].lodging_details.hotel_name")
                .and_then(ReportValue::as_text),
            Some("MARINA BAY GRAND HOTEL")
        );
    }

    #[test]
    fn renders_canonical_bundle_to_json() {
        let bundle = synthesize_bundle(&synthetic_docs());
        let rendered = render_canonical_bundle_json_pretty(&bundle).expect("bundle should render");
        assert!(rendered.contains("\"expense_lines\""));
        assert!(rendered.contains("\"generic_receipt\""));
    }

    #[test]
    fn projection_marks_missing_usd_conversion_as_bundle_warning() {
        let result = synthesize_bundle_projection(&synthetic_docs());
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::MissingUsdConversion
                && issue.severity == BundleIssueSeverity::Warning));
    }

    #[test]
    fn projection_contains_manual_review_warnings_for_low_confidence_defaults() {
        let result = synthesize_bundle_projection(&synthetic_docs());
        assert!(result
            .draft
            .metadata
            .get("expense_report.transaction_lines[0].common.foreign_activity_type")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(result
            .draft
            .metadata
            .get("expense_report.transaction_lines[1].common.foreign_activity_type")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(!result
            .validation
            .issues
            .iter()
            .any(|issue| issue.kind == ValidationIssueKind::LowConfidenceWithoutReview
                && (issue.path
                    == "expense_report.transaction_lines[0].common.foreign_activity_type"
                    || issue.path
                        == "expense_report.transaction_lines[1].common.foreign_activity_type")));
    }
}
