use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::document_facts::{
    DateRange, DocumentFactsPayload, ExtractedDocumentFacts, FlightItineraryFacts, HotelFolioFacts,
    Location, MoneyAmount, Observed, ReceiptFacts,
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
    Meal,
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
pub struct CanonicalMealDetails {
    pub venue_name: Option<Observed<String>>,
    pub tip_amount: Option<Observed<String>>,
    pub alcohol_amount: Option<Observed<String>>,
    pub has_alcohol_on_receipt: Option<Observed<bool>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalExpenseLine {
    pub line_id: String,
    pub kind: CanonicalExpenseKind,
    pub document_id: String,
    pub date: Option<Observed<String>>,
    pub line_amount_usd: Option<Observed<String>>,
    pub exchange_rate: Option<Observed<String>>,
    pub original_currency: Option<Observed<String>>,
    pub original_amount: Option<Observed<String>>,
    pub expense_type: Option<Observed<String>>,
    pub remarks: Option<Observed<String>>,
    pub country_of_activity: Option<Observed<String>>,
    pub foreign_activity_type: Option<Observed<String>>,
    pub source_documents: Vec<BundleSourceDocument>,
    pub airfare_details: Option<CanonicalAirfareDetails>,
    pub lodging_details: Option<CanonicalLodgingDetails>,
    pub meal_details: Option<CanonicalMealDetails>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FxRateQuote {
    pub currency: String,
    pub date: String,
    pub usd_per_unit: String,
    pub evidence: Vec<EvidenceReference>,
}

pub trait FxRateProvider {
    fn usd_rate_for(&self, currency: &str, date: &str) -> Option<FxRateQuote>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticFxRateProvider {
    rates: BTreeMap<(String, String), FxRateQuote>,
}

impl StaticFxRateProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_rate(
        &mut self,
        currency: impl Into<String>,
        date: impl Into<String>,
        usd_per_unit: impl Into<String>,
        evidence: Vec<EvidenceReference>,
    ) {
        let currency = currency.into().to_ascii_uppercase();
        let date = date.into();
        self.rates.insert(
            (currency.clone(), date.clone()),
            FxRateQuote {
                currency,
                date,
                usd_per_unit: usd_per_unit.into(),
                evidence,
            },
        );
    }

    pub fn demo() -> Self {
        let mut provider = Self::new();
        let demo_evidence = vec![EvidenceReference {
            kind: EvidenceKind::SystemGenerated,
            document_id: None,
            filename: None,
            page: None,
            quote: None,
            origin: Some("bundle_synthesis.demo_fx_rate_provider".to_owned()),
        }];

        for (date, rates) in [
            (
                "2018-10-01",
                [("MYR", "0.24"), ("SGD", "0.73"), ("JPY", "0.0088")].as_slice(),
            ),
            (
                "2018-12-01",
                [("MYR", "0.24"), ("SGD", "0.73"), ("JPY", "0.0089")].as_slice(),
            ),
            (
                "2019-01-01",
                [("MYR", "0.24"), ("SGD", "0.74"), ("JPY", "0.0091")].as_slice(),
            ),
            (
                "2019-10-01",
                [("MYR", "0.24"), ("SGD", "0.73"), ("JPY", "0.0093")].as_slice(),
            ),
            (
                "2025-04-01",
                [
                    ("SGD", "0.74"),
                    ("MYR", "0.23"),
                    ("JPY", "0.0067"),
                    ("GBP", "1.25"),
                    ("EUR", "1.08"),
                    ("CAD", "0.73"),
                    ("AUD", "0.66"),
                ]
                .as_slice(),
            ),
        ] {
            for (currency, rate) in rates {
                provider.insert_rate(*currency, date, *rate, demo_evidence.clone());
            }
        }
        provider
    }
}

impl FxRateProvider for StaticFxRateProvider {
    fn usd_rate_for(&self, currency: &str, date: &str) -> Option<FxRateQuote> {
        let currency = canonical_currency_code(currency);
        let date = normalize_bundle_date(date).unwrap_or_else(|| date.to_owned());
        self.rates
            .get(&(currency.clone(), date.to_owned()))
            .cloned()
            .or_else(|| {
                self.rates
                    .range((currency.clone(), String::new())..=(currency, date.to_owned()))
                    .next_back()
                    .map(|(_, quote)| quote.clone())
            })
    }
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
    let expense_lines = synthesize_expense_lines(documents, &trip);
    issues.extend(collect_line_level_issues(&expense_lines));

    CanonicalExpenseBundle {
        documents: documents.to_vec(),
        payee,
        trip,
        expense_lines,
        issues,
    }
}

pub fn enrich_bundle_with_fx(
    bundle: &mut CanonicalExpenseBundle,
    fx_rate_provider: &dyn FxRateProvider,
) {
    for line in &mut bundle.expense_lines {
        enrich_line_with_fx(line, fx_rate_provider);
    }

    bundle.issues.retain(|issue| match issue.kind {
        BundleIssueKind::MissingUsdConversion => issue.document_ids.iter().any(|document_id| {
            bundle
                .expense_lines
                .iter()
                .find(|line| &line.document_id == document_id)
                .is_some_and(|line| line.line_amount_usd.is_none())
        }),
        _ => true,
    });
}

pub fn project_bundle_to_draft(bundle: &CanonicalExpenseBundle) -> (DraftReport, Vec<BundleIssue>) {
    let mut metadata = BTreeMap::new();
    let mut projection_issues = Vec::new();

    let mut general_information = BTreeMap::new();
    let category = bundle.trip.region.as_ref().map(category_from_region);
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

    let business_purpose_who = bundle.payee.as_ref().map(|payee| payee.name.clone());
    let business_purpose_when = bundle.trip.window.as_ref().map(|window| {
        system_observed(
            format!("{} to {}", window.value.start_date, window.value.end_date),
            window.confidence,
            window.evidence.clone(),
            "bundle_synthesis.project_business_purpose_when",
            window.flags.clone(),
        )
    });
    let business_purpose_where = bundle.trip.destination.as_ref().map(|destination| {
        system_observed(
            location_to_display(&destination.value),
            destination.confidence,
            destination.evidence.clone(),
            "bundle_synthesis.project_business_purpose_where",
            destination.flags.clone(),
        )
    });
    let business_purpose_what = synthesize_business_purpose_what(bundle);
    let business_purpose_why = synthesize_business_purpose_why(bundle);
    let business_purpose_key = synthesize_business_purpose_key(
        business_purpose_who.as_ref(),
        business_purpose_what.as_ref(),
        category.as_ref(),
    );

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
    if let Some(payee_name) = business_purpose_who.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.who",
            "who",
            payee_name,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(when) = business_purpose_when.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.when",
            "when",
            when,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(what) = business_purpose_what.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.what",
            "what",
            what,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(where_value) = business_purpose_where.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.where",
            "where",
            where_value,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(why) = business_purpose_why.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.why",
            "why",
            why,
            |value| ReportValue::String(value.clone()),
        );
    }

    if let Some(key_30char) = business_purpose_key.as_ref() {
        insert_observed_leaf(
            &mut business_purpose,
            &mut metadata,
            "expense_report.general_information.business_purpose.key_30char",
            "key_30char",
            key_30char,
            |value| ReportValue::String(value.clone()),
        );
    }

    if !business_purpose.is_empty() {
        general_information.insert(
            "business_purpose".to_owned(),
            ReportValue::Object(business_purpose),
        );
    }

    if let Some(event_name) = synthesize_event_name(bundle, category.as_ref()) {
        insert_observed_leaf(
            &mut general_information,
            &mut metadata,
            "expense_report.general_information.event_name",
            "event_name",
            &event_name,
            |value| ReportValue::String(value.clone()),
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
            "Bundle cannot compute total_usd until every projected line has a USD amount"
                .to_owned(),
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
    report.insert(
        "allocation_and_approvers".to_owned(),
        ReportValue::Object(BTreeMap::from([(
            "other_beneficiaries".to_owned(),
            ReportValue::Bool(false),
        )])),
    );
    metadata.insert(
        "expense_report.allocation_and_approvers.other_beneficiaries".to_owned(),
        field_metadata_from_observed(&system_observed(
            false,
            ConfidenceLevel::Low,
            Vec::new(),
            "bundle_synthesis.default_other_beneficiaries_false",
            vec!["requires_beneficiary_review".to_owned()],
        )),
    );

    let draft = DraftReport {
        report: ReportValue::Object(report),
        metadata,
    };
    (draft, projection_issues)
}

pub fn synthesize_bundle_projection(
    documents: &[ExtractedDocumentFacts],
) -> BundleProjectionResult {
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

pub fn synthesize_bundle_projection_with_fx(
    documents: &[ExtractedDocumentFacts],
    fx_rate_provider: &dyn FxRateProvider,
) -> BundleProjectionResult {
    let mut bundle = synthesize_bundle(documents);
    enrich_bundle_with_fx(&mut bundle, fx_rate_provider);
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
            DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                traveler_names, ..
            }) => {
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
    let mut receipt_dates = Vec::new();

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
            DocumentFactsPayload::Receipt(ReceiptFacts {
                transaction_date: Some(date),
                ..
            }) => receipt_dates.push(date.clone()),
            _ => {}
        }
    }

    let candidates = if !flight_windows.is_empty() {
        flight_windows
    } else {
        fallback_windows
    };

    let Some((chosen, _)) = candidates.first().cloned() else {
        if let Some(receipt_window) = infer_trip_window_from_receipt_dates(&receipt_dates) {
            return Some(receipt_window);
        }
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
        if let Some(inferred) = infer_destination_from_currency(documents) {
            return Some(inferred);
        }
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
) -> Vec<CanonicalExpenseLine> {
    let mut lines = Vec::new();

    for document in documents {
        match &document.facts {
            DocumentFactsPayload::FlightItinerary(facts) => {
                lines.push(synthesize_airfare_line(document, facts, trip));
            }
            DocumentFactsPayload::HotelFolio(facts) => {
                lines.push(synthesize_lodging_line(document, facts, trip));
            }
            DocumentFactsPayload::Receipt(facts) => {
                lines.push(synthesize_receipt_line(document, facts, trip));
            }
            _ => {}
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
            facts
                .segments
                .first()
                .map(|segment| segment.departure_airport.value.clone())
                .unwrap_or_else(|| "unknown origin".to_owned()),
            facts
                .segments
                .last()
                .map(|segment| segment.arrival_airport.value.clone())
                .unwrap_or_else(|| "unknown destination".to_owned())
        ),
        ConfidenceLevel::Medium,
        amount
            .map(|value| value.evidence.clone())
            .unwrap_or_default(),
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
    let departure_airport = facts
        .departure_location
        .as_ref()
        .and_then(|location| {
            location.value.airport_code.as_ref().map(|value| {
                system_observed(
                    value.clone(),
                    location.confidence,
                    location.evidence.clone(),
                    "bundle_synthesis.project_departure_airport_from_location",
                    location.flags.clone(),
                )
            })
        })
        .or_else(|| first_segment.map(|segment| clone_string_observed(&segment.departure_airport)));
    let destination_airport = facts
        .arrival_location
        .as_ref()
        .and_then(|location| {
            location.value.airport_code.as_ref().map(|value| {
                system_observed(
                    value.clone(),
                    location.confidence,
                    location.evidence.clone(),
                    "bundle_synthesis.project_destination_airport_from_location",
                    location.flags.clone(),
                )
            })
        })
        .or_else(|| first_segment.map(|segment| clone_string_observed(&segment.arrival_airport)))
        .or_else(|| last_segment.map(|segment| clone_string_observed(&segment.arrival_airport)));
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
        departure_airport,
        destination_airport,
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
        exchange_rate: identity_exchange_rate_if_usd(amount, trip),
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
        meal_details: None,
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
        .or_else(|| {
            facts
                .nightly_charges
                .last()
                .map(|night| clone_string_observed(&night.date))
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
            facts
                .property_name
                .as_ref()
                .map(|value| value.value.clone())
                .unwrap_or_else(|| "unknown hotel".to_owned())
        ),
        ConfidenceLevel::Medium,
        amount
            .map(|value| value.evidence.clone())
            .unwrap_or_default(),
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
                facts
                    .nightly_charges
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
        exchange_rate: identity_exchange_rate_if_usd(amount, trip),
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
        meal_details: None,
        projection_supported: true,
    }
}

fn synthesize_receipt_line(
    document: &ExtractedDocumentFacts,
    facts: &ReceiptFacts,
    trip: &CanonicalTrip,
) -> CanonicalExpenseLine {
    if looks_like_meal_receipt(facts) {
        return synthesize_meal_line(document, facts, trip);
    }

    CanonicalExpenseLine {
        line_id: format!("{}::receipt", document.document_id),
        kind: CanonicalExpenseKind::GenericReceipt,
        document_id: document.document_id.clone(),
        date: facts.transaction_date.as_ref().map(clone_string_observed),
        line_amount_usd: facts.total_paid.as_ref().and_then(usd_amount_from_money),
        exchange_rate: identity_exchange_rate_if_usd(facts.total_paid.as_ref(), trip),
        original_currency: facts
            .total_paid
            .as_ref()
            .and_then(currency_observed_from_money),
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
        meal_details: None,
        projection_supported: false,
    }
}

fn synthesize_meal_line(
    document: &ExtractedDocumentFacts,
    facts: &ReceiptFacts,
    trip: &CanonicalTrip,
) -> CanonicalExpenseLine {
    let amount = facts.total_paid.as_ref();
    let has_alcohol = receipt_has_alcohol(facts);
    let alcohol_amount = sum_receipt_items_by_keywords(
        facts,
        &[
            "beer",
            "wine",
            "cocktail",
            "whiskey",
            "vodka",
            "gin",
            "ale",
            "lager",
            "champagne",
        ],
    );
    let expense_type = trip.region.as_ref().map_or_else(
        || {
            Some(system_observed(
                if has_alcohol {
                    "business_meal_with_alcohol".to_owned()
                } else {
                    "business_meal".to_owned()
                },
                ConfidenceLevel::Medium,
                amount
                    .map(|value| value.evidence.clone())
                    .unwrap_or_default(),
                "bundle_synthesis.heuristic_meal_classification",
                vec!["heuristic_receipt_classification".to_owned()],
            ))
        },
        |_| {
            Some(system_observed(
                if has_alcohol {
                    "business_meal_with_alcohol".to_owned()
                } else {
                    "business_meal".to_owned()
                },
                ConfidenceLevel::Medium,
                amount
                    .map(|value| value.evidence.clone())
                    .unwrap_or_default(),
                "bundle_synthesis.heuristic_meal_classification",
                vec!["heuristic_receipt_classification".to_owned()],
            ))
        },
    );
    let remarks = facts.merchant_name.as_ref().map(|merchant| {
        system_observed(
            format!("Meal at {}", merchant.value),
            ConfidenceLevel::Medium,
            merchant.evidence.clone(),
            "bundle_synthesis.build_meal_remarks",
            vec!["heuristic_receipt_classification".to_owned()],
        )
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

    CanonicalExpenseLine {
        line_id: format!("{}::meal", document.document_id),
        kind: CanonicalExpenseKind::Meal,
        document_id: document.document_id.clone(),
        date: facts.transaction_date.as_ref().map(clone_string_observed),
        line_amount_usd: amount.and_then(usd_amount_from_money),
        exchange_rate: identity_exchange_rate_if_usd(amount, trip),
        original_currency: amount.and_then(currency_observed_from_money),
        original_amount: amount.map(number_observed_from_money),
        expense_type,
        remarks,
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
        foreign_activity_type,
        source_documents: vec![BundleSourceDocument {
            document_id: document.document_id.clone(),
            filename: document.filename.clone(),
            document_type: "receipt".to_owned(),
        }],
        airfare_details: None,
        lodging_details: None,
        meal_details: Some(CanonicalMealDetails {
            venue_name: facts.merchant_name.as_ref().map(clone_string_observed),
            tip_amount: facts.tip_amount.as_ref().map(number_observed_from_money),
            alcohol_amount,
            has_alcohol_on_receipt: Some(system_observed(
                has_alcohol,
                ConfidenceLevel::Medium,
                facts
                    .line_items
                    .iter()
                    .flat_map(|item| item.description.evidence.clone())
                    .collect(),
                "bundle_synthesis.detect_receipt_alcohol",
                if has_alcohol {
                    vec!["heuristic_receipt_classification".to_owned()]
                } else {
                    Vec::new()
                },
            )),
        }),
        projection_supported: true,
    }
}

fn project_transaction_lines(
    bundle: &CanonicalExpenseBundle,
    metadata: &mut BTreeMap<String, FieldMetadata>,
    issues: &mut Vec<BundleIssue>,
) -> Vec<ReportValue> {
    let mut lines = Vec::new();

    for line in bundle
        .expense_lines
        .iter()
        .filter(|line| line.projection_supported)
    {
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
        if let Some(exchange_rate) = line.exchange_rate.as_ref() {
            insert_observed_leaf(
                &mut common,
                metadata,
                &format!("{base_path}.common.exchange_rate"),
                "exchange_rate",
                exchange_rate,
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
                airfare_details.insert("price_comparison".to_owned(), ReportValue::empty_object());
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
            CanonicalExpenseKind::Meal => {
                let mut meal_details = BTreeMap::new();
                if let Some(details) = line.meal_details.as_ref() {
                    if let Some(venue_name) = details.venue_name.as_ref() {
                        insert_observed_leaf(
                            &mut meal_details,
                            metadata,
                            &format!("{base_path}.meal_details.venue_name"),
                            "venue_name",
                            venue_name,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(meal_purpose) = synthesize_meal_purpose(bundle, line).as_ref() {
                        insert_observed_leaf(
                            &mut meal_details,
                            metadata,
                            &format!("{base_path}.meal_details.meal_purpose"),
                            "meal_purpose",
                            meal_purpose,
                            |value| ReportValue::String(value.clone()),
                        );
                    }
                    if let Some(tip_amount) = details.tip_amount.as_ref() {
                        insert_observed_leaf(
                            &mut meal_details,
                            metadata,
                            &format!("{base_path}.meal_details.tip_amount"),
                            "tip_amount",
                            tip_amount,
                            |value| ReportValue::Number(value.clone()),
                        );
                    }
                    if let Some(alcohol_amount) = details.alcohol_amount.as_ref() {
                        insert_observed_leaf(
                            &mut meal_details,
                            metadata,
                            &format!("{base_path}.meal_details.alcohol_amount"),
                            "alcohol_amount",
                            alcohol_amount,
                            |value| ReportValue::Number(value.clone()),
                        );
                    }
                    if let Some(has_alcohol_on_receipt) = details.has_alcohol_on_receipt.as_ref() {
                        insert_observed_leaf(
                            &mut meal_details,
                            metadata,
                            &format!("{base_path}.meal_details.has_alcohol_on_receipt"),
                            "has_alcohol_on_receipt",
                            has_alcohol_on_receipt,
                            |value| ReportValue::Bool(*value),
                        );
                    }
                }
                line_object.insert("meal_details".to_owned(), ReportValue::Object(meal_details));
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

    let lines_for_total = if supported_lines.is_empty() {
        bundle.expense_lines.iter().collect::<Vec<_>>()
    } else {
        supported_lines
    };

    if lines_for_total.is_empty()
        || lines_for_total
            .iter()
            .any(|line| line.line_amount_usd.is_none())
    {
        return None;
    }

    let mut evidence = Vec::new();
    let mut total_cents = 0i64;
    for line in &lines_for_total {
        let amount = line.line_amount_usd.as_ref()?;
        total_cents += amount_to_cents(&amount.value)?;
        evidence.extend(amount.evidence.clone());
    }

    let includes_unprojected = lines_for_total
        .iter()
        .any(|line| !line.projection_supported);

    Some(system_observed(
        cents_to_amount(total_cents),
        if includes_unprojected {
            ConfidenceLevel::Medium
        } else {
            ConfidenceLevel::High
        },
        evidence,
        if includes_unprojected {
            "bundle_synthesis.compute_total_usd_from_all_lines"
        } else {
            "bundle_synthesis.compute_total_usd"
        },
        if includes_unprojected {
            vec!["includes_unprojected_documents".to_owned()]
        } else {
            Vec::new()
        },
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

fn synthesize_business_purpose_what(bundle: &CanonicalExpenseBundle) -> Option<Observed<String>> {
    if let Some(summary_what) = first_summary_business_purpose_what(bundle) {
        return Some(system_observed(
            summary_what.value.clone(),
            summary_what.confidence,
            summary_what.evidence.clone(),
            "bundle_synthesis.project_business_purpose_what_from_summary",
            summary_what.flags.clone(),
        ));
    }

    if let Some(event_name) = first_document_event_name(bundle) {
        return Some(system_observed(
            event_name.value.clone(),
            event_name.confidence,
            event_name.evidence.clone(),
            "bundle_synthesis.project_business_purpose_what_from_event_name",
            event_name.flags.clone(),
        ));
    }

    bundle.trip.destination.as_ref().map(|destination| {
        system_observed(
            format!(
                "Business travel to {}",
                location_to_display(&destination.value)
            ),
            ConfidenceLevel::Low,
            destination.evidence.clone(),
            "bundle_synthesis.default_business_purpose_what",
            vec!["requires_purpose_review".to_owned()],
        )
    })
}

fn synthesize_business_purpose_why(bundle: &CanonicalExpenseBundle) -> Option<Observed<String>> {
    if let Some(summary_why) = first_summary_business_purpose_why(bundle) {
        return Some(system_observed(
            summary_why.value.clone(),
            summary_why.confidence,
            summary_why.evidence.clone(),
            "bundle_synthesis.project_business_purpose_why_from_summary",
            summary_why.flags.clone(),
        ));
    }

    if let Some(event_name) = first_document_event_name(bundle) {
        return Some(system_observed(
            format!("Participation in {}", event_name.value),
            ConfidenceLevel::Low,
            event_name.evidence.clone(),
            "bundle_synthesis.default_business_purpose_why_from_event_name",
            with_review_flag(&event_name.flags, "requires_purpose_review"),
        ));
    }

    bundle.trip.destination.as_ref().map(|destination| {
        system_observed(
            format!(
                "Travel and related business expenses for work in {}",
                location_to_display(&destination.value)
            ),
            ConfidenceLevel::Low,
            destination.evidence.clone(),
            "bundle_synthesis.default_business_purpose_why",
            vec!["requires_purpose_review".to_owned()],
        )
    })
}

fn synthesize_business_purpose_key(
    who: Option<&Observed<String>>,
    what: Option<&Observed<String>>,
    category: Option<&Observed<String>>,
) -> Option<Observed<String>> {
    let who = who?;
    let what = what?;
    let category = category?;

    let key = truncate_chars(
        &format!(
            "{}{}{}",
            compact_key_token(&who.value, 10),
            compact_key_token(&what.value, 12),
            compact_key_token(&category.value, 8),
        ),
        30,
    );

    Some(system_observed(
        key,
        lowest_confidence([who.confidence, what.confidence, category.confidence]),
        combined_evidence([
            who.evidence.as_slice(),
            what.evidence.as_slice(),
            category.evidence.as_slice(),
        ]),
        "bundle_synthesis.compute_business_purpose_key",
        combined_flags([
            who.flags.as_slice(),
            what.flags.as_slice(),
            category.flags.as_slice(),
        ]),
    ))
}

fn synthesize_event_name(
    bundle: &CanonicalExpenseBundle,
    category: Option<&Observed<String>>,
) -> Option<Observed<String>> {
    if let Some(event_name) = first_document_event_name(bundle) {
        return Some(system_observed(
            event_name.value.clone(),
            event_name.confidence,
            event_name.evidence.clone(),
            "bundle_synthesis.project_event_name_from_document",
            event_name.flags.clone(),
        ));
    }

    let category = category?;
    let event_name = match category.value.as_str() {
        "expenses_foreign" => "Foreign Expenses",
        "expenses_domestic" => "Domestic Expenses",
        _ => "Expense Report",
    };

    Some(system_observed(
        event_name.to_owned(),
        ConfidenceLevel::Low,
        category.evidence.clone(),
        "bundle_synthesis.default_event_name_from_category",
        with_review_flag(&category.flags, "requires_event_name_review"),
    ))
}

fn synthesize_meal_purpose(
    bundle: &CanonicalExpenseBundle,
    line: &CanonicalExpenseLine,
) -> Option<Observed<String>> {
    if let Some(event_name) = first_document_event_name(bundle) {
        return Some(system_observed(
            format!("Meal during {}", event_name.value),
            ConfidenceLevel::Low,
            event_name.evidence.clone(),
            "bundle_synthesis.default_meal_purpose_from_event_name",
            with_review_flag(&event_name.flags, "requires_meal_purpose_review"),
        ));
    }

    if let Some(country) = line.country_of_activity.as_ref() {
        return Some(system_observed(
            format!("Business meal during travel in {}", country.value),
            ConfidenceLevel::Low,
            country.evidence.clone(),
            "bundle_synthesis.default_meal_purpose_from_activity_country",
            with_review_flag(&country.flags, "requires_meal_purpose_review"),
        ));
    }

    bundle.trip.destination.as_ref().map(|destination| {
        system_observed(
            format!(
                "Business meal during travel to {}",
                location_to_display(&destination.value)
            ),
            ConfidenceLevel::Low,
            destination.evidence.clone(),
            "bundle_synthesis.default_meal_purpose_from_trip_destination",
            vec!["requires_meal_purpose_review".to_owned()],
        )
    })
}

fn first_document_event_name(bundle: &CanonicalExpenseBundle) -> Option<&Observed<String>> {
    for document in &bundle.documents {
        match &document.facts {
            DocumentFactsPayload::ConferenceRegistration(facts) => {
                if let Some(event_name) = facts.event_name.as_ref() {
                    return Some(event_name);
                }
            }
            DocumentFactsPayload::ConferenceProgram(facts) => {
                if let Some(event_name) = facts.event_name.as_ref() {
                    return Some(event_name);
                }
            }
            DocumentFactsPayload::StanfordExpenseSummary(facts) => {
                if let Some(event_name) = facts.event_name.as_ref() {
                    return Some(event_name);
                }
            }
            _ => {}
        }
    }

    None
}

fn first_summary_business_purpose_what(
    bundle: &CanonicalExpenseBundle,
) -> Option<&Observed<String>> {
    bundle
        .documents
        .iter()
        .find_map(|document| match &document.facts {
            DocumentFactsPayload::StanfordExpenseSummary(facts) => {
                facts.business_purpose_what.as_ref()
            }
            _ => None,
        })
}

fn first_summary_business_purpose_why(
    bundle: &CanonicalExpenseBundle,
) -> Option<&Observed<String>> {
    bundle
        .documents
        .iter()
        .find_map(|document| match &document.facts {
            DocumentFactsPayload::StanfordExpenseSummary(facts) => {
                facts.business_purpose_why.as_ref()
            }
            _ => None,
        })
}

fn collect_line_level_issues(lines: &[CanonicalExpenseLine]) -> Vec<BundleIssue> {
    let mut issues = Vec::new();

    for line in lines {
        if line.projection_supported && line.line_amount_usd.is_none() {
            issues.push(bundle_issue(
                BundleIssueSeverity::Warning,
                BundleIssueKind::MissingUsdConversion,
                format!(
                    "Document {} needs FX enrichment before it can produce a complete projected line",
                    line.document_id
                ),
                vec![line.document_id.clone()],
                line
                    .original_amount
                    .as_ref()
                    .map(|amount| amount.evidence.clone())
                    .unwrap_or_default(),
            ));
        }

        if !line.projection_supported {
            issues.push(bundle_issue(
                BundleIssueSeverity::Warning,
                BundleIssueKind::UnprojectedDocument,
                format!(
                    "Document {} remains in the bundle but is not yet projected into schema transaction lines",
                    line.document_id
                ),
                vec![line.document_id.clone()],
                line
                    .original_amount
                    .as_ref()
                    .map(|amount| amount.evidence.clone())
                    .unwrap_or_default(),
            ));
        }
    }

    issues
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

fn identity_exchange_rate_if_usd(
    amount: Option<&Observed<MoneyAmount>>,
    trip: &CanonicalTrip,
) -> Option<Observed<String>> {
    trip.region
        .as_ref()
        .filter(|region| region.value == TravelRegion::Foreign)
        .and_then(|_| amount)
        .and_then(|amount| {
            amount
                .value
                .currency
                .as_deref()
                .filter(|currency| currency.eq_ignore_ascii_case("USD"))
                .map(|_| {
                    system_observed(
                        "1.00".to_owned(),
                        amount.confidence,
                        amount.evidence.clone(),
                        "bundle_synthesis.identity_exchange_rate_for_usd",
                        amount.flags.clone(),
                    )
                })
        })
}

fn looks_like_meal_receipt(facts: &ReceiptFacts) -> bool {
    facts.merchant_name.as_ref().is_some_and(|merchant| {
        contains_any(
            &merchant.value,
            &[
                "restaurant",
                "bistro",
                "cafe",
                "coffee",
                "bar",
                "grill",
                "kitchen",
                "diner",
                "noodle",
                "pizza",
                "burger",
                "steak",
            ],
        )
    }) || facts.line_items.iter().any(|item| {
        contains_any(
            &item.description.value,
            &[
                "breakfast",
                "lunch",
                "dinner",
                "coffee",
                "tea",
                "meal",
                "laksa",
                "sandwich",
                "salad",
                "soup",
                "service charge",
            ],
        )
    })
}

fn receipt_has_alcohol(facts: &ReceiptFacts) -> bool {
    facts.line_items.iter().any(|item| {
        contains_any(
            &item.description.value,
            &[
                "beer",
                "wine",
                "cocktail",
                "whiskey",
                "vodka",
                "gin",
                "ale",
                "lager",
                "champagne",
            ],
        )
    })
}

fn sum_receipt_items_by_keywords(
    facts: &ReceiptFacts,
    keywords: &[&str],
) -> Option<Observed<String>> {
    let matching_items = facts
        .line_items
        .iter()
        .filter(|item| contains_any(&item.description.value, keywords))
        .collect::<Vec<_>>();
    if matching_items.is_empty() {
        return None;
    }

    let mut total_cents = 0i64;
    let mut evidence = Vec::new();
    for item in matching_items {
        total_cents += amount_to_cents(&item.amount.value.amount)?;
        evidence.extend(item.amount.evidence.clone());
    }

    Some(system_observed(
        cents_to_amount(total_cents),
        ConfidenceLevel::High,
        evidence,
        "bundle_synthesis.sum_receipt_item_subset",
        Vec::new(),
    ))
}

fn enrich_line_with_fx(line: &mut CanonicalExpenseLine, fx_rate_provider: &dyn FxRateProvider) {
    if line.line_amount_usd.is_some() && line.exchange_rate.is_some() {
        return;
    }

    let Some(date) = line.date.as_ref() else {
        return;
    };
    let Some(original_currency) = line.original_currency.as_ref() else {
        return;
    };
    let Some(original_amount) = line.original_amount.as_ref() else {
        return;
    };

    if original_currency.value.eq_ignore_ascii_case("USD") {
        if line.exchange_rate.is_none() {
            line.exchange_rate = Some(system_observed(
                "1.00".to_owned(),
                original_currency.confidence,
                original_currency.evidence.clone(),
                "bundle_synthesis.identity_exchange_rate_for_usd",
                original_currency.flags.clone(),
            ));
        }
        if line.line_amount_usd.is_none() {
            line.line_amount_usd = Some(system_observed(
                original_amount.value.clone(),
                original_amount.confidence,
                original_amount.evidence.clone(),
                "bundle_synthesis.project_usd_amount_from_original",
                original_amount.flags.clone(),
            ));
        }
        return;
    }

    let Some(quote) = fx_rate_provider.usd_rate_for(&original_currency.value, &date.value) else {
        return;
    };

    let Some(converted_amount) = multiply_amounts(&original_amount.value, &quote.usd_per_unit)
    else {
        return;
    };

    let mut exchange_evidence = quote.evidence.clone();
    exchange_evidence.extend(original_currency.evidence.clone());
    let exchange_rate = system_observed(
        quote.usd_per_unit.clone(),
        ConfidenceLevel::High,
        exchange_evidence,
        "bundle_synthesis.apply_fx_rate",
        Vec::new(),
    );

    let mut amount_evidence = quote.evidence.clone();
    amount_evidence.extend(original_amount.evidence.clone());
    let line_amount_usd = system_observed(
        converted_amount,
        ConfidenceLevel::High,
        amount_evidence,
        "bundle_synthesis.convert_original_amount_to_usd",
        Vec::new(),
    );

    line.exchange_rate = Some(exchange_rate);
    line.line_amount_usd = Some(line_amount_usd);
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    let normalized = normalize_text(value);
    needles
        .iter()
        .any(|needle| normalized.contains(&normalize_text(needle)))
}

fn multiply_amounts(lhs: &str, rhs: &str) -> Option<String> {
    let lhs_cents = amount_to_cents(lhs)?;
    let rhs_basis_points = amount_to_basis_points(rhs)?;
    let scaled = lhs_cents.checked_mul(rhs_basis_points)?;
    let usd_cents = (scaled + 5_000) / 10_000;
    Some(cents_to_amount(usd_cents))
}

fn amount_to_basis_points(amount: &str) -> Option<i64> {
    let (whole, fraction) = amount.split_once('.')?;
    let whole = whole.parse::<i64>().ok()?;
    let fraction = format!("{:0<4}", fraction);
    let fraction = fraction.get(0..4)?.parse::<i64>().ok()?;
    Some(whole * 10_000 + fraction)
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
    compatible_optional_text(lhs.city.as_deref(), rhs.city.as_deref())
        && compatible_optional_text(lhs.country.as_deref(), rhs.country.as_deref())
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.map(normalize_text)
}

fn compatible_optional_text(lhs: Option<&str>, rhs: Option<&str>) -> bool {
    match (normalize_optional_text(lhs), normalize_optional_text(rhs)) {
        (Some(lhs), Some(rhs)) => lhs == rhs,
        (Some(_), None) | (None, Some(_)) | (None, None) => true,
    }
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

fn canonical_currency_code(value: &str) -> String {
    match value.to_ascii_uppercase().as_str() {
        "RM" => "MYR".to_owned(),
        other => other.to_owned(),
    }
}

fn infer_trip_window_from_receipt_dates(
    receipt_dates: &[Observed<String>],
) -> Option<Observed<DateRange>> {
    let normalized_dates = receipt_dates
        .iter()
        .filter_map(|date| normalize_bundle_date(&date.value).map(|normalized| (date, normalized)))
        .collect::<Vec<_>>();
    let start = normalized_dates
        .iter()
        .min_by_key(|(_, normalized)| normalized.clone())?;
    let end = normalized_dates
        .iter()
        .max_by_key(|(_, normalized)| normalized.clone())?;
    let mut evidence = start.0.evidence.clone();
    evidence.extend(end.0.evidence.clone());
    Some(system_observed(
        DateRange {
            start_date: start.1.clone(),
            end_date: end.1.clone(),
        },
        lowest_confidence([start.0.confidence, end.0.confidence, ConfidenceLevel::Low]),
        evidence,
        "bundle_synthesis.infer_trip_window_from_receipt_dates",
        vec!["receipt_only_inference".to_owned()],
    ))
}

fn normalize_bundle_date(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let separator = if trimmed.contains('/') {
        '/'
    } else if trimmed.contains('-') {
        '-'
    } else {
        return None;
    };
    let parts = trimmed
        .split(separator)
        .map(|part| part.trim())
        .collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }

    if parts[0].len() == 4 {
        let year = parts[0].parse::<u32>().ok()?;
        let month = parts[1].parse::<u32>().ok()?;
        let day = parts[2].parse::<u32>().ok()?;
        return format_iso_date(year, month, day);
    }

    let mut day = parts[0].parse::<u32>().ok()?;
    let mut month = parts[1].parse::<u32>().ok()?;
    let year = parse_bundle_year(parts[2])?;

    if day <= 12 && month > 12 {
        std::mem::swap(&mut day, &mut month);
    }

    format_iso_date(year, month, day)
}

fn parse_bundle_year(value: &str) -> Option<u32> {
    let year = value.parse::<u32>().ok()?;
    Some(if value.len() == 2 {
        if year >= 70 {
            1900 + year
        } else {
            2000 + year
        }
    } else {
        year
    })
}

fn format_iso_date(year: u32, month: u32, day: u32) -> Option<String> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn infer_destination_from_currency(
    documents: &[ExtractedDocumentFacts],
) -> Option<Observed<Location>> {
    let mut inferred_countries = Vec::new();
    let mut evidence = Vec::new();

    for document in documents {
        let amount = match &document.facts {
            DocumentFactsPayload::FlightItinerary(facts) => facts.total_paid.as_ref(),
            DocumentFactsPayload::HotelFolio(facts) => facts.total_paid.as_ref(),
            DocumentFactsPayload::Receipt(facts) => facts.total_paid.as_ref(),
            _ => None,
        };
        let Some(amount) = amount else {
            continue;
        };
        let Some(currency) = amount.value.currency.as_deref() else {
            continue;
        };
        let Some(country) = currency_implied_country(currency) else {
            continue;
        };
        inferred_countries.push(country);
        evidence.extend(amount.evidence.clone());
    }

    let first = inferred_countries.first()?;
    if inferred_countries.iter().any(|country| country != first) {
        return None;
    }

    Some(system_observed(
        Location {
            city: None,
            region: None,
            country: Some((*first).to_owned()),
            airport_code: None,
        },
        ConfidenceLevel::Low,
        evidence,
        "bundle_synthesis.infer_destination_from_currency",
        vec!["currency_only_destination".to_owned()],
    ))
}

fn currency_implied_country(currency: &str) -> Option<&'static str> {
    match canonical_currency_code(currency).as_str() {
        "MYR" => Some("Malaysia"),
        "SGD" => Some("Singapore"),
        "JPY" => Some("Japan"),
        "GBP" => Some("United Kingdom"),
        "CAD" => Some("Canada"),
        "AUD" => Some("Australia"),
        _ => None,
    }
}

fn compact_key_token(value: &str, max_chars: usize) -> String {
    truncate_chars(
        &value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .map(|ch| ch.to_ascii_uppercase())
            .collect::<String>(),
        max_chars,
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn lowest_confidence(values: impl IntoIterator<Item = ConfidenceLevel>) -> ConfidenceLevel {
    values
        .into_iter()
        .fold(ConfidenceLevel::High, |lowest, current| {
            match (lowest, current) {
                (ConfidenceLevel::Low, _) | (_, ConfidenceLevel::Low) => ConfidenceLevel::Low,
                (ConfidenceLevel::Medium, _) | (_, ConfidenceLevel::Medium) => {
                    ConfidenceLevel::Medium
                }
                _ => ConfidenceLevel::High,
            }
        })
}

fn combined_evidence<'a>(
    groups: impl IntoIterator<Item = &'a [EvidenceReference]>,
) -> Vec<EvidenceReference> {
    groups
        .into_iter()
        .flat_map(|group| group.iter().cloned())
        .collect()
}

fn combined_flags<'a>(groups: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    let mut flags = Vec::new();
    for group in groups {
        for flag in group {
            if !flags.contains(flag) {
                flags.push(flag.clone());
            }
        }
    }
    flags
}

fn with_review_flag(flags: &[String], review_flag: &str) -> Vec<String> {
    let mut combined = flags.to_vec();
    if !combined.iter().any(|flag| flag == review_flag) {
        combined.push(review_flag.to_owned());
    }
    combined
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
    use crate::synthetic_documents::{generate_synthetic_document, SyntheticVariant};
    use crate::validator::{ValidationIssueKind, ValidationSeverity};
    use crate::DocumentKind;
    use crate::{DocumentClassification, ExtractionStatus};

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

    fn sample_receipt_only_docs() -> Vec<ExtractedDocumentFacts> {
        let make_receipt = |document_id: &str,
                            filename: &str,
                            merchant: &str,
                            date: &str,
                            amount: &str,
                            currency: &str| ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::Receipt,
                confidence: ConfidenceLevel::Medium,
                evidence: vec![document_reference(document_id, filename)],
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(Observed::new(
                    merchant.to_owned(),
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                merchant_location: None,
                transaction_date: Some(Observed::new(
                    date.to_owned(),
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                total_paid: Some(Observed::new(
                    MoneyAmount {
                        amount: amount.to_owned(),
                        currency: Some(currency.to_owned()),
                    },
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                subtotal: None,
                tax_amount: None,
                tip_amount: None,
                line_items: Vec::new(),
            }),
            issues: Vec::new(),
        };

        vec![
            make_receipt(
                "x00016469612",
                "x00016469612.png",
                "BOOK TALK",
                "2018-12-25",
                "9.00",
                "MYR",
            ),
            make_receipt(
                "x00016469619",
                "x00016469619.png",
                "INDAH GIFT & HOME DECO",
                "2019-01-19",
                "60.30",
                "MYR",
            ),
        ]
    }

    fn sample_receipt_only_docs_with_raw_dates() -> Vec<ExtractedDocumentFacts> {
        let mut documents = sample_receipt_only_docs();
        let DocumentFactsPayload::Receipt(first_receipt) = &mut documents[0].facts else {
            panic!("expected receipt facts");
        };
        first_receipt.transaction_date.as_mut().unwrap().value = "25/12/2018".to_owned();

        let DocumentFactsPayload::Receipt(second_receipt) = &mut documents[1].facts else {
            panic!("expected receipt facts");
        };
        second_receipt.transaction_date.as_mut().unwrap().value = "12-01-19".to_owned();

        documents
    }

    fn sample_receipt_only_docs_live_sroie_style() -> Vec<ExtractedDocumentFacts> {
        let mut documents = sample_receipt_only_docs_with_raw_dates();
        let DocumentFactsPayload::Receipt(second_receipt) = &mut documents[1].facts else {
            panic!("expected receipt facts");
        };
        second_receipt.transaction_date.as_mut().unwrap().value = "19/10/2018".to_owned();

        let make_receipt = |document_id: &str,
                            filename: &str,
                            merchant: &str,
                            date: &str,
                            amount: &str,
                            currency: &str| ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::Receipt,
                confidence: ConfidenceLevel::Medium,
                evidence: vec![document_reference(document_id, filename)],
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(Observed::new(
                    merchant.to_owned(),
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                merchant_location: None,
                transaction_date: Some(Observed::new(
                    date.to_owned(),
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                total_paid: Some(Observed::new(
                    MoneyAmount {
                        amount: amount.to_owned(),
                        currency: Some(currency.to_owned()),
                    },
                    ConfidenceLevel::Medium,
                    vec![document_reference(document_id, filename)],
                )),
                subtotal: None,
                tax_amount: None,
                tip_amount: None,
                line_items: Vec::new(),
            }),
            issues: Vec::new(),
        };

        documents.push(make_receipt(
            "x00016469620",
            "x00016469620.png",
            "MR D.I.Y. (JOHOR) SDN BHD",
            "12-01-19",
            "33.90",
            "MYR",
        ));
        documents
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

        assert_eq!(
            bundle.payee.as_ref().map(|payee| payee.name.value.as_str()),
            Some("Olivia Park")
        );
        assert_eq!(
            bundle
                .trip
                .destination
                .as_ref()
                .and_then(|destination| destination.value.country.as_deref()),
            Some("Singapore")
        );
        assert_eq!(
            bundle.trip.region.as_ref().map(|region| region.value),
            Some(TravelRegion::Foreign)
        );
        assert_eq!(bundle.expense_lines.len(), 3);
        assert!(bundle
            .expense_lines
            .iter()
            .any(|line| line.kind == CanonicalExpenseKind::Meal));
        assert!(!bundle
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
            get_path(&result.draft.report, "general_information.category")
                .and_then(ReportValue::as_text),
            Some("expenses_foreign")
        );
        assert_eq!(
            get_path(&result.draft.report, "general_information.rush_processing")
                .and_then(ReportValue::as_text),
            Some("no")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.transaction_type")
                .and_then(ReportValue::as_text),
            Some("foreign")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[0].common.expense_type"
            )
            .and_then(ReportValue::as_text),
            Some("airfare_foreign")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[1].common.expense_type"
            )
            .and_then(ReportValue::as_text),
            Some("lodging_foreign")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[2].common.expense_type"
            )
            .and_then(ReportValue::as_text),
            Some("business_meal")
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
        assert_eq!(
            get_path(
                &result.draft.report,
                "general_information.business_purpose.what"
            )
            .and_then(ReportValue::as_text),
            Some("Business travel to Singapore")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "general_information.business_purpose.why"
            )
            .and_then(ReportValue::as_text),
            Some("Travel and related business expenses for work in Singapore")
        );
        assert_eq!(
            get_path(&result.draft.report, "general_information.event_name")
                .and_then(ReportValue::as_text),
            Some("Foreign Expenses")
        );
        assert!(result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.general_information.payee.affiliation"));
        assert!(result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_summary.total_usd"));
        assert!(result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_lines[1].common.line_amount_usd"));
        assert!(result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && issue.path == "expense_report.transaction_lines[2].meal_details.attendees"));
        assert!(!result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && (issue.path == "expense_report.general_information.business_purpose.what"
                || issue.path == "expense_report.general_information.business_purpose.why"
                || issue.path
                    == "expense_report.general_information.business_purpose.key_30char"
                || issue.path == "expense_report.general_information.event_name"
                || issue.path == "expense_report.transaction_lines[2].meal_details.meal_purpose"
                || issue.path == "expense_report.allocation_and_approvers")));
    }

    #[test]
    fn projected_foreign_airfare_uses_identity_usd_exchange_context() {
        let result = synthesize_bundle_projection(&synthetic_docs());

        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[0].common.original_currency"
            )
            .and_then(ReportValue::as_text),
            Some("USD")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[0].common.original_amount"
            )
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
            result
                .bundle
                .payee
                .as_ref()
                .map(|payee| payee.name.value.as_str()),
            Some("Olivia Park")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[0].airfare_details.airline"
            )
            .and_then(ReportValue::as_text),
            Some("ANA")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[1].lodging_details.hotel_name"
            )
            .and_then(ReportValue::as_text),
            Some("MARINA BAY GRAND HOTEL")
        );
        assert!(!result
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::ConflictingDestination));
    }

    #[test]
    fn curated_bundle_projects_itinerary_destination_airport() {
        let documents = vec![
            curated_sidecar("flight_itinerary/airline_itinerary_classic.md.expected.json"),
            curated_sidecar("hotel_folio/hotel_folio_guest_bill.md.expected.json"),
        ];

        let result = synthesize_bundle_projection(&documents);
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[0].airfare_details.destination_airport"
            )
            .and_then(ReportValue::as_text),
            Some("SIN")
        );
    }

    #[test]
    fn renders_canonical_bundle_to_json() {
        let bundle = synthesize_bundle(&synthetic_docs());
        let rendered = render_canonical_bundle_json_pretty(&bundle).expect("bundle should render");
        assert!(rendered.contains("\"expense_lines\""));
        assert!(rendered.contains("\"meal\""));
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
        assert!(result
            .draft
            .metadata
            .get("expense_report.general_information.business_purpose.what")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(result
            .draft
            .metadata
            .get("expense_report.general_information.business_purpose.why")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(result
            .draft
            .metadata
            .get("expense_report.general_information.event_name")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(result
            .draft
            .metadata
            .get("expense_report.allocation_and_approvers.other_beneficiaries")
            .is_some_and(|metadata| metadata.needs_review));
        assert!(!result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::LowConfidenceWithoutReview
            && (issue.path == "expense_report.transaction_lines[0].common.foreign_activity_type"
                || issue.path
                    == "expense_report.transaction_lines[1].common.foreign_activity_type")));
    }

    #[test]
    fn classifies_restaurant_receipt_into_meal_projection() {
        let result = synthesize_bundle_projection(&synthetic_docs());
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[2].meal_details.venue_name"
            )
            .and_then(ReportValue::as_text),
            Some("East Bay Bistro")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[2].meal_details.tip_amount"
            )
            .and_then(ReportValue::as_text),
            Some("4.50")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[2].meal_details.has_alcohol_on_receipt"
            ),
            Some(&ReportValue::Bool(false))
        );
        assert!(result
            .draft
            .metadata
            .get("expense_report.transaction_lines[2].common.expense_type")
            .is_some_and(|metadata| metadata.needs_review));
    }

    #[test]
    fn unknown_receipt_remains_unprojected() {
        let mut documents = synthetic_docs();
        let DocumentFactsPayload::Receipt(facts) = &mut documents[2].facts else {
            panic!("expected receipt facts");
        };
        facts.merchant_name.as_mut().unwrap().value = "Global Services Pte Ltd".to_owned();
        facts.line_items.clear();

        let bundle = synthesize_bundle(&documents);
        assert!(bundle
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::UnprojectedDocument));
        assert!(bundle
            .expense_lines
            .iter()
            .any(|line| line.kind == CanonicalExpenseKind::GenericReceipt));
    }

    #[test]
    fn receipt_only_bundle_infers_trip_window_and_destination_from_receipt_context() {
        let bundle = synthesize_bundle(&sample_receipt_only_docs());

        assert_eq!(
            bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.start_date.as_str()),
            Some("2018-12-25")
        );
        assert_eq!(
            bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.end_date.as_str()),
            Some("2019-01-19")
        );
        assert_eq!(
            bundle
                .trip
                .destination
                .as_ref()
                .and_then(|destination| destination.value.country.as_deref()),
            Some("Malaysia")
        );
        assert_eq!(
            bundle.trip.region.as_ref().map(|region| region.value),
            Some(TravelRegion::Foreign)
        );
        assert!(bundle
            .issues
            .iter()
            .all(|issue| issue.kind != BundleIssueKind::MissingDestination
                && issue.kind != BundleIssueKind::MissingTripWindow));
        assert!(bundle
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::MissingPayeeName));
    }

    #[test]
    fn static_fx_provider_uses_latest_prior_rate_for_historical_dates() {
        let provider = StaticFxRateProvider::demo();
        let quote = provider
            .usd_rate_for("RM", "2018-12-25")
            .expect("historical MYR rate should resolve from prior anchor");
        assert_eq!(quote.currency, "MYR");
        assert_eq!(quote.usd_per_unit, "0.24");
        assert_eq!(quote.date, "2018-12-01");
    }

    #[test]
    fn static_fx_provider_normalizes_non_iso_receipt_dates() {
        let provider = StaticFxRateProvider::demo();
        let first = provider
            .usd_rate_for("MYR", "25/12/2018")
            .expect("slash-formatted historical date should resolve");
        let second = provider
            .usd_rate_for("MYR", "12-01-19")
            .expect("two-digit hyphenated receipt date should resolve");
        let third = provider
            .usd_rate_for("MYR", "19/10/2018")
            .expect("older slash-formatted date should resolve against earlier anchor");
        assert_eq!(first.usd_per_unit, "0.24");
        assert_eq!(first.date, "2018-12-01");
        assert_eq!(second.usd_per_unit, "0.24");
        assert_eq!(second.date, "2019-01-01");
        assert_eq!(third.usd_per_unit, "0.24");
        assert_eq!(third.date, "2018-10-01");
    }

    #[test]
    fn fx_enrichment_computes_total_usd_for_receipt_only_bundle() {
        let provider = StaticFxRateProvider::demo();
        let result = synthesize_bundle_projection_with_fx(&sample_receipt_only_docs(), &provider);

        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.total_usd")
                .and_then(ReportValue::as_text),
            Some("16.63")
        );
        assert!(result
            .bundle
            .expense_lines
            .iter()
            .all(|line| line.line_amount_usd.is_some() && line.exchange_rate.is_some()));
        assert!(result
            .bundle
            .expense_lines
            .iter()
            .all(|line| !line.projection_supported));
        assert!(result
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::UnprojectedDocument));
        assert!(!result
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::MissingTransactionSummaryTotal));
    }

    #[test]
    fn receipt_only_bundle_with_raw_dates_still_infers_destination_and_total_usd() {
        let provider = StaticFxRateProvider::demo();
        let result = synthesize_bundle_projection_with_fx(
            &sample_receipt_only_docs_with_raw_dates(),
            &provider,
        );

        assert_eq!(
            result
                .bundle
                .trip
                .destination
                .as_ref()
                .and_then(|destination| destination.value.country.as_deref()),
            Some("Malaysia")
        );
        assert_eq!(
            result
                .bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.start_date.as_str()),
            Some("2018-12-25")
        );
        assert_eq!(
            result
                .bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.end_date.as_str()),
            Some("2019-01-12")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.total_usd")
                .and_then(ReportValue::as_text),
            Some("16.63")
        );
        assert!(result
            .issues
            .iter()
            .all(|issue| issue.kind != BundleIssueKind::MissingDestination
                && issue.kind != BundleIssueKind::MissingTransactionSummaryTotal));
    }

    #[test]
    fn live_style_receipt_bundle_computes_total_usd_across_three_raw_receipts() {
        let provider = StaticFxRateProvider::demo();
        let result = synthesize_bundle_projection_with_fx(
            &sample_receipt_only_docs_live_sroie_style(),
            &provider,
        );

        assert_eq!(
            result
                .bundle
                .trip
                .destination
                .as_ref()
                .and_then(|destination| destination.value.country.as_deref()),
            Some("Malaysia")
        );
        assert_eq!(
            result
                .bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.start_date.as_str()),
            Some("2018-10-19")
        );
        assert_eq!(
            result
                .bundle
                .trip
                .window
                .as_ref()
                .map(|window| window.value.end_date.as_str()),
            Some("2019-01-12")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.total_usd")
                .and_then(ReportValue::as_text),
            Some("24.77")
        );
        assert!(result
            .bundle
            .expense_lines
            .iter()
            .all(|line| line.line_amount_usd.is_some() && line.exchange_rate.is_some()));
        assert!(result
            .issues
            .iter()
            .all(|issue| issue.kind != BundleIssueKind::MissingDestination
                && issue.kind != BundleIssueKind::MissingTransactionSummaryTotal));
    }

    #[test]
    fn fx_enrichment_converts_foreign_lines_and_reduces_validation_gaps() {
        let provider = StaticFxRateProvider::demo();
        let result = synthesize_bundle_projection_with_fx(&synthetic_docs(), &provider);

        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[1].common.exchange_rate"
            )
            .and_then(ReportValue::as_text),
            Some("0.74")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[1].common.line_amount_usd"
            )
            .and_then(ReportValue::as_text),
            Some("576.31")
        );
        assert_eq!(
            get_path(
                &result.draft.report,
                "transaction_lines[2].common.line_amount_usd"
            )
            .and_then(ReportValue::as_text),
            Some("25.91")
        );
        assert_eq!(
            get_path(&result.draft.report, "transaction_summary.total_usd")
                .and_then(ReportValue::as_text),
            Some("1889.66")
        );
        assert!(!result
            .issues
            .iter()
            .any(|issue| issue.kind == BundleIssueKind::MissingUsdConversion));
        assert!(!result.validation.issues.iter().any(|issue| issue.kind
            == ValidationIssueKind::MissingRequiredField
            && (issue.path == "expense_report.transaction_lines[1].common.line_amount_usd"
                || issue.path == "expense_report.transaction_lines[1].common.exchange_rate"
                || issue.path == "expense_report.transaction_lines[2].common.line_amount_usd"
                || issue.path == "expense_report.transaction_lines[2].common.exchange_rate"
                || issue.path == "expense_report.transaction_summary.total_usd")));
        let remaining_error_paths = result
            .validation
            .issues
            .iter()
            .filter(|issue| issue.severity == ValidationSeverity::Error)
            .map(|issue| issue.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            remaining_error_paths,
            vec![
                "expense_report.general_information.payee.affiliation",
                "expense_report.general_information",
                "expense_report.general_information.authorized_by",
                "expense_report.transaction_lines[2].meal_details.attendees",
            ]
        );
    }
}
