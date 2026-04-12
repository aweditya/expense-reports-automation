use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::draft::{ConfidenceLevel, EvidenceReference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    FlightItinerary,
    HotelFolio,
    Receipt,
    ConferenceRegistration,
    ConferenceProgram,
    CurrencyConversion,
    AirfarePriceComparison,
    MissingReceiptDeclaration,
    StanfordExpenseSummary,
    Unknown,
}

impl DocumentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FlightItinerary => "flight_itinerary",
            Self::HotelFolio => "hotel_folio",
            Self::Receipt => "receipt",
            Self::ConferenceRegistration => "conference_registration",
            Self::ConferenceProgram => "conference_program",
            Self::CurrencyConversion => "currency_conversion",
            Self::AirfarePriceComparison => "airfare_price_comparison",
            Self::MissingReceiptDeclaration => "missing_receipt_declaration",
            Self::StanfordExpenseSummary => "stanford_expense_summary",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStatus {
    Complete,
    Partial,
    NeedsReview,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observed<T> {
    pub value: T,
    pub confidence: ConfidenceLevel,
    pub evidence: Vec<EvidenceReference>,
    pub flags: Vec<String>,
}

impl<T> Observed<T> {
    pub fn new(value: T, confidence: ConfidenceLevel, evidence: Vec<EvidenceReference>) -> Self {
        Self {
            value,
            confidence,
            evidence,
            flags: Vec::new(),
        }
    }

    pub fn needs_review(&self) -> bool {
        self.confidence == ConfidenceLevel::Low || !self.flags.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentClassification {
    pub kind: DocumentKind,
    pub confidence: ConfidenceLevel,
    pub evidence: Vec<EvidenceReference>,
    pub flags: Vec<String>,
}

impl DocumentClassification {
    pub fn needs_review(&self) -> bool {
        self.confidence == ConfidenceLevel::Low || !self.flags.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentExtractionIssue {
    pub severity: IssueSeverity,
    pub code: String,
    pub message: String,
    pub evidence: Vec<EvidenceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoneyAmount {
    pub amount: String,
    pub currency: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub airport_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightSegmentFacts {
    pub departure_airport: Observed<String>,
    pub arrival_airport: Observed<String>,
    pub departure_date: Observed<String>,
    pub arrival_date: Option<Observed<String>>,
    pub marketing_carrier: Option<Observed<String>>,
    pub flight_number: Option<Observed<String>>,
    pub cabin_class: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlightItineraryFacts {
    pub traveler_names: Vec<Observed<String>>,
    pub confirmation_code: Option<Observed<String>>,
    pub ticket_number: Option<Observed<String>>,
    pub booking_date: Option<Observed<String>>,
    pub trip_window: Option<Observed<DateRange>>,
    pub departure_location: Option<Observed<Location>>,
    pub arrival_location: Option<Observed<Location>>,
    pub segments: Vec<FlightSegmentFacts>,
    pub total_paid: Option<Observed<MoneyAmount>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotelNightChargeFacts {
    pub date: Observed<String>,
    pub room_rate: Option<Observed<MoneyAmount>>,
    pub taxes_and_fees: Vec<Observed<MoneyAmount>>,
    pub description: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HotelFolioFacts {
    pub guest_name: Option<Observed<String>>,
    pub property_name: Option<Observed<String>>,
    pub folio_number: Option<Observed<String>>,
    pub stay_window: Option<Observed<DateRange>>,
    pub property_location: Option<Observed<Location>>,
    pub nightly_charges: Vec<HotelNightChargeFacts>,
    pub total_paid: Option<Observed<MoneyAmount>>,
    pub meals_included: Vec<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptLineItemFacts {
    pub description: Observed<String>,
    pub amount: Observed<MoneyAmount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptFacts {
    pub merchant_name: Option<Observed<String>>,
    pub merchant_location: Option<Observed<Location>>,
    pub transaction_date: Option<Observed<String>>,
    pub total_paid: Option<Observed<MoneyAmount>>,
    pub subtotal: Option<Observed<MoneyAmount>>,
    pub tax_amount: Option<Observed<MoneyAmount>>,
    pub tip_amount: Option<Observed<MoneyAmount>>,
    pub line_items: Vec<ReceiptLineItemFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConferenceRegistrationFacts {
    pub attendee_name: Option<Observed<String>>,
    pub event_name: Option<Observed<String>>,
    pub event_window: Option<Observed<DateRange>>,
    pub organizer_name: Option<Observed<String>>,
    pub registration_type: Option<Observed<String>>,
    pub total_paid: Option<Observed<MoneyAmount>>,
    pub included_meals: Vec<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationFact {
    pub title: Observed<String>,
    pub presentation_date: Option<Observed<String>>,
    pub presenters: Vec<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConferenceProgramFacts {
    pub attendee_name: Option<Observed<String>>,
    pub event_name: Option<Observed<String>>,
    pub event_window: Option<Observed<DateRange>>,
    pub presentations: Vec<PresentationFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrencyConversionFacts {
    pub conversion_date: Option<Observed<String>>,
    pub provider_name: Option<Observed<String>>,
    pub source_amount: Option<Observed<MoneyAmount>>,
    pub target_amount: Option<Observed<MoneyAmount>>,
    pub exchange_rate: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirfarePriceComparisonFacts {
    pub comparison_date: Option<Observed<String>>,
    pub selected_fare: Option<Observed<MoneyAmount>>,
    pub lowest_logical_fare: Option<Observed<MoneyAmount>>,
    pub policy_outcome: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingReceiptDeclarationFacts {
    pub claimant_name: Option<Observed<String>>,
    pub merchant_name: Option<Observed<String>>,
    pub expense_date: Option<Observed<String>>,
    pub amount: Option<Observed<MoneyAmount>>,
    pub explanation: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StanfordExpenseSummaryFacts {
    pub payee_name: Option<Observed<String>>,
    pub event_name: Option<Observed<String>>,
    pub category: Option<Observed<String>>,
    pub expense_date_window: Option<Observed<DateRange>>,
    pub submitted_on: Option<Observed<String>>,
    pub transaction_number: Option<Observed<String>>,
    pub payment_method: Option<Observed<String>>,
    pub reimbursement_amount: Option<Observed<MoneyAmount>>,
    pub status: Option<Observed<String>>,
    pub business_purpose_who: Option<Observed<String>>,
    pub business_purpose_what: Option<Observed<String>>,
    pub business_purpose_when: Option<Observed<String>>,
    pub business_purpose_where: Option<Observed<String>>,
    pub business_purpose_why: Option<Observed<String>>,
    pub authorized_by: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnknownDocumentFacts {
    pub title_hint: Option<Observed<String>>,
    pub text_summary: Option<Observed<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentFactsPayload {
    FlightItinerary(FlightItineraryFacts),
    HotelFolio(HotelFolioFacts),
    Receipt(ReceiptFacts),
    ConferenceRegistration(ConferenceRegistrationFacts),
    ConferenceProgram(ConferenceProgramFacts),
    CurrencyConversion(CurrencyConversionFacts),
    AirfarePriceComparison(AirfarePriceComparisonFacts),
    MissingReceiptDeclaration(MissingReceiptDeclarationFacts),
    StanfordExpenseSummary(StanfordExpenseSummaryFacts),
    Unknown(UnknownDocumentFacts),
}

impl DocumentFactsPayload {
    pub fn kind(&self) -> DocumentKind {
        match self {
            Self::FlightItinerary(_) => DocumentKind::FlightItinerary,
            Self::HotelFolio(_) => DocumentKind::HotelFolio,
            Self::Receipt(_) => DocumentKind::Receipt,
            Self::ConferenceRegistration(_) => DocumentKind::ConferenceRegistration,
            Self::ConferenceProgram(_) => DocumentKind::ConferenceProgram,
            Self::CurrencyConversion(_) => DocumentKind::CurrencyConversion,
            Self::AirfarePriceComparison(_) => DocumentKind::AirfarePriceComparison,
            Self::MissingReceiptDeclaration(_) => DocumentKind::MissingReceiptDeclaration,
            Self::StanfordExpenseSummary(_) => DocumentKind::StanfordExpenseSummary,
            Self::Unknown(_) => DocumentKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedDocumentFacts {
    pub document_id: String,
    pub filename: String,
    pub classification: DocumentClassification,
    pub extraction_status: ExtractionStatus,
    pub facts: DocumentFactsPayload,
    pub issues: Vec<DocumentExtractionIssue>,
}

impl ExtractedDocumentFacts {
    pub fn needs_review(&self) -> bool {
        self.extraction_status == ExtractionStatus::NeedsReview
            || self.classification.needs_review()
            || self
                .issues
                .iter()
                .any(|issue| issue.severity == IssueSeverity::Error)
    }

    pub fn validate_contract(&self) -> Result<(), DocumentFactContractError> {
        let payload_kind = self.facts.kind();
        if self.classification.kind != payload_kind {
            return Err(DocumentFactContractError::ClassificationPayloadMismatch {
                classification_kind: self.classification.kind,
                payload_kind,
            });
        }

        if self.classification.evidence.is_empty() {
            return Err(DocumentFactContractError::MissingClassificationEvidence);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentFactContractError {
    ClassificationPayloadMismatch {
        classification_kind: DocumentKind,
        payload_kind: DocumentKind,
    },
    MissingClassificationEvidence,
}

impl fmt::Display for DocumentFactContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassificationPayloadMismatch {
                classification_kind,
                payload_kind,
            } => write!(
                f,
                "document fact contract mismatch: classification is {}, payload is {}",
                classification_kind.as_str(),
                payload_kind.as_str()
            ),
            Self::MissingClassificationEvidence => {
                write!(f, "document classification must include at least one evidence reference")
            }
        }
    }
}

impl std::error::Error for DocumentFactContractError {}

pub fn render_document_facts_json_pretty(
    facts: &ExtractedDocumentFacts,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(facts)
}

pub fn parse_document_facts_json_str(
    value: &str,
) -> Result<ExtractedDocumentFacts, serde_json::Error> {
    serde_json::from_str(value)
}

pub fn parse_document_facts_json_path(
    path: impl AsRef<Path>,
) -> Result<ExtractedDocumentFacts, Box<dyn std::error::Error>> {
    let value = std::fs::read_to_string(path)?;
    Ok(parse_document_facts_json_str(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draft::EvidenceKind;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_evidence() -> EvidenceReference {
        EvidenceReference {
            kind: EvidenceKind::DocumentSpan,
            document_id: Some("doc_001".to_owned()),
            filename: Some("sample.pdf".to_owned()),
            page: Some(1),
            quote: Some("Flight Confirmation".to_owned()),
            origin: None,
        }
    }

    fn write_temp_json(contents: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("expense_report_document_facts_{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be creatable");
        let path = dir.join("facts.json");
        fs::write(&path, contents).expect("json fixture should be writable");
        path
    }

    #[test]
    fn validates_matching_classification_and_payload() {
        let facts = ExtractedDocumentFacts {
            document_id: "doc_001".to_owned(),
            filename: "sample.pdf".to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::FlightItinerary,
                confidence: ConfidenceLevel::High,
                evidence: vec![sample_evidence()],
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                traveler_names: vec![Observed::new(
                    "Olivia".to_owned(),
                    ConfidenceLevel::High,
                    vec![sample_evidence()],
                )],
                confirmation_code: None,
                ticket_number: None,
                booking_date: None,
                trip_window: None,
                departure_location: None,
                arrival_location: None,
                segments: Vec::new(),
                total_paid: None,
            }),
            issues: Vec::new(),
        };

        assert!(facts.validate_contract().is_ok());
        assert!(!facts.needs_review());
    }

    #[test]
    fn rejects_mismatched_classification_and_payload() {
        let facts = ExtractedDocumentFacts {
            document_id: "doc_001".to_owned(),
            filename: "sample.pdf".to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::Receipt,
                confidence: ConfidenceLevel::High,
                evidence: vec![sample_evidence()],
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                guest_name: None,
                property_name: None,
                folio_number: None,
                stay_window: None,
                property_location: None,
                nightly_charges: Vec::new(),
                total_paid: None,
                meals_included: Vec::new(),
            }),
            issues: Vec::new(),
        };

        assert_eq!(
            facts.validate_contract(),
            Err(DocumentFactContractError::ClassificationPayloadMismatch {
                classification_kind: DocumentKind::Receipt,
                payload_kind: DocumentKind::HotelFolio,
            })
        );
    }

    #[test]
    fn low_confidence_classification_marks_document_for_review() {
        let facts = ExtractedDocumentFacts {
            document_id: "doc_001".to_owned(),
            filename: "sample.pdf".to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::Unknown,
                confidence: ConfidenceLevel::Low,
                evidence: vec![sample_evidence()],
                flags: vec!["ambiguous_layout".to_owned()],
            },
            extraction_status: ExtractionStatus::NeedsReview,
            facts: DocumentFactsPayload::Unknown(UnknownDocumentFacts {
                title_hint: None,
                text_summary: None,
            }),
            issues: vec![DocumentExtractionIssue {
                severity: IssueSeverity::Warning,
                code: "ambiguous_layout".to_owned(),
                message: "Document could be either a receipt or a folio".to_owned(),
                evidence: vec![sample_evidence()],
            }],
        };

        assert!(facts.validate_contract().is_ok());
        assert!(facts.needs_review());
    }

    #[test]
    fn parses_rendered_document_facts_from_path() {
        let facts = ExtractedDocumentFacts {
            document_id: "doc_001".to_owned(),
            filename: "sample.pdf".to_owned(),
            classification: DocumentClassification {
                kind: DocumentKind::Receipt,
                confidence: ConfidenceLevel::High,
                evidence: vec![sample_evidence()],
                flags: Vec::new(),
            },
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(Observed::new(
                    "East Bay Bistro".to_owned(),
                    ConfidenceLevel::High,
                    vec![sample_evidence()],
                )),
                merchant_location: None,
                transaction_date: Some(Observed::new(
                    "2025-04-24".to_owned(),
                    ConfidenceLevel::High,
                    vec![sample_evidence()],
                )),
                total_paid: None,
                subtotal: None,
                tax_amount: None,
                tip_amount: None,
                line_items: Vec::new(),
            }),
            issues: Vec::new(),
        };
        let rendered = render_document_facts_json_pretty(&facts).expect("facts should render");
        let path = write_temp_json(&rendered);
        let reparsed = parse_document_facts_json_path(&path).expect("facts should parse from path");

        assert_eq!(reparsed, facts);
        fs::remove_dir_all(path.parent().unwrap()).expect("temp dir should be removable");
    }
}
