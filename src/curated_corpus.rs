use std::path::{Path, PathBuf};

use crate::document_extract::extract_document_facts_path;
use crate::document_facts::{
    render_document_facts_json_pretty, DateRange, DocumentClassification, DocumentFactsPayload,
    DocumentKind, ExtractedDocumentFacts, ExtractionStatus, FlightItineraryFacts,
    FlightSegmentFacts, HotelFolioFacts, HotelNightChargeFacts, Location, MoneyAmount, Observed,
    ReceiptFacts, ReceiptLineItemFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedCorpusCase {
    pub id: &'static str,
    pub relative_path: &'static str,
    pub expected_facts: ExtractedDocumentFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedCorpusFailure {
    pub case_id: String,
    pub relative_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CuratedCorpusVerificationReport {
    pub failures: Vec<CuratedCorpusFailure>,
}

impl CuratedCorpusVerificationReport {
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

pub fn curated_corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/curated")
}

pub fn curated_corpus_cases() -> Vec<CuratedCorpusCase> {
    vec![
        airline_itinerary_classic_case(),
        airline_itinerary_trip_window_case(),
        hotel_folio_guest_bill_case(),
        hotel_folio_property_labeled_case(),
        receipt_card_dotted_case(),
        receipt_merchant_labeled_case(),
    ]
}

pub fn verify_curated_corpus() -> CuratedCorpusVerificationReport {
    let mut failures = Vec::new();

    for case in curated_corpus_cases() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(case.relative_path);
        match extract_document_facts_path(&path) {
            Ok(actual) => {
                if actual != case.expected_facts {
                    let expected_json = render_document_facts_json_pretty(&case.expected_facts)
                        .unwrap_or_else(|err| format!("unable to render expected facts: {err}"));
                    let actual_json = render_document_facts_json_pretty(&actual)
                        .unwrap_or_else(|err| format!("unable to render actual facts: {err}"));
                    failures.push(CuratedCorpusFailure {
                        case_id: case.id.to_owned(),
                        relative_path: case.relative_path.to_owned(),
                        message: format!("facts mismatch\nEXPECTED:\n{expected_json}\nACTUAL:\n{actual_json}"),
                    });
                } else if let Err(err) = actual.validate_contract() {
                    failures.push(CuratedCorpusFailure {
                        case_id: case.id.to_owned(),
                        relative_path: case.relative_path.to_owned(),
                        message: format!("contract validation failed: {err}"),
                    });
                }
            }
            Err(err) => failures.push(CuratedCorpusFailure {
                case_id: case.id.to_owned(),
                relative_path: case.relative_path.to_owned(),
                message: format!("extraction failed: {err}"),
            }),
        }
    }

    CuratedCorpusVerificationReport { failures }
}

fn airline_itinerary_classic_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/flight_itinerary/airline_itinerary_classic.md";
    let filename = "airline_itinerary_classic.md";
    let document_id = "airline_itinerary_classic";
    CuratedCorpusCase {
        id: "airline_itinerary_classic",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(
                DocumentKind::FlightItinerary,
                document_id,
                filename,
                "E-TICKET ITINERARY RECEIPT",
            ),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                traveler_names: vec![observed(
                    "Olivia Park".to_owned(),
                    document_id,
                    filename,
                    "Passenger Olivia Park",
                )],
                confirmation_code: Some(observed(
                    "H7K9Q2".to_owned(),
                    document_id,
                    filename,
                    "Record Locator H7K9Q2",
                )),
                ticket_number: Some(observed(
                    "0162459135784".to_owned(),
                    document_id,
                    filename,
                    "Ticket Number 0162459135784",
                )),
                booking_date: Some(observed(
                    "2025-04-10".to_owned(),
                    document_id,
                    filename,
                    "Issue Date 2025-04-10",
                )),
                trip_window: Some(Observed {
                    value: DateRange {
                        start_date: "2025-04-21".to_owned(),
                        end_date: "2025-04-29".to_owned(),
                    },
                    confidence: ConfidenceLevel::Medium,
                    evidence: vec![
                        evidence(document_id, filename, "Departure 2025-04-21"),
                        evidence(document_id, filename, "Return 2025-04-29"),
                        system_evidence("document_extract.infer_trip_window_from_departure_return"),
                    ],
                    flags: Vec::new(),
                }),
                departure_location: Some(observed(
                    Location {
                        city: Some("San Francisco".to_owned()),
                        region: Some("CA".to_owned()),
                        country: Some("United States".to_owned()),
                        airport_code: Some("SFO".to_owned()),
                    },
                    document_id,
                    filename,
                    "From San Francisco, CA, United States (SFO)",
                )),
                arrival_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: None,
                        airport_code: Some("SIN".to_owned()),
                    },
                    document_id,
                    filename,
                    "To Singapore (SIN)",
                )),
                segments: vec![
                    flight_segment(
                        document_id,
                        filename,
                        "1 | SFO -> NRT | 2025-04-21 -> 2025-04-22 | ANA NH107 | Economy",
                        "SFO",
                        "NRT",
                        "2025-04-21",
                        "2025-04-22",
                        "ANA",
                        "NH107",
                        "Economy",
                    ),
                    flight_segment(
                        document_id,
                        filename,
                        "2 | SIN -> SFO | 2025-04-29 -> 2025-04-29 | ANA NH108 | Economy",
                        "SIN",
                        "SFO",
                        "2025-04-29",
                        "2025-04-29",
                        "ANA",
                        "NH108",
                        "Economy",
                    ),
                ],
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "1287.44".to_owned(),
                        currency: Some("USD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Total Fare Paid USD 1,287.44",
                )),
            }),
            issues: Vec::new(),
        },
    }
}

fn airline_itinerary_trip_window_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/flight_itinerary/airline_itinerary_trip_window.md";
    let filename = "airline_itinerary_trip_window.md";
    let document_id = "airline_itinerary_trip_window";
    CuratedCorpusCase {
        id: "airline_itinerary_trip_window",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(
                DocumentKind::FlightItinerary,
                document_id,
                filename,
                "Air Travel Itinerary Receipt",
            ),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                traveler_names: vec![observed(
                    "Olivia Park".to_owned(),
                    document_id,
                    filename,
                    "Passenger Name: Olivia Park",
                )],
                confirmation_code: Some(observed(
                    "H7K9Q2".to_owned(),
                    document_id,
                    filename,
                    "Confirmation Code: H7K9Q2",
                )),
                ticket_number: Some(observed(
                    "0162459135784".to_owned(),
                    document_id,
                    filename,
                    "Ticket No: 0162459135784",
                )),
                booking_date: Some(observed(
                    "2025-04-10".to_owned(),
                    document_id,
                    filename,
                    "Booking Date: 2025-04-10",
                )),
                trip_window: Some(observed(
                    DateRange {
                        start_date: "2025-04-21".to_owned(),
                        end_date: "2025-04-29".to_owned(),
                    },
                    document_id,
                    filename,
                    "Trip Window: 2025-04-21 to 2025-04-29",
                )),
                departure_location: Some(observed(
                    Location {
                        city: Some("San Francisco".to_owned()),
                        region: Some("CA".to_owned()),
                        country: Some("United States".to_owned()),
                        airport_code: Some("SFO".to_owned()),
                    },
                    document_id,
                    filename,
                    "Origin: San Francisco, CA, United States (SFO)",
                )),
                arrival_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: None,
                        airport_code: Some("SIN".to_owned()),
                    },
                    document_id,
                    filename,
                    "Destination: Singapore (SIN)",
                )),
                segments: vec![
                    flight_segment(
                        document_id,
                        filename,
                        "Segment 1 | Departure Airport: SFO | Arrival Airport: NRT | Departure Date: 2025-04-21 | Arrival Date: 2025-04-22 | Marketing Carrier: ANA | Flight Number: NH107 | Cabin Class: Economy",
                        "SFO",
                        "NRT",
                        "2025-04-21",
                        "2025-04-22",
                        "ANA",
                        "NH107",
                        "Economy",
                    ),
                    flight_segment(
                        document_id,
                        filename,
                        "Segment 2 | Departure Airport: SIN | Arrival Airport: SFO | Departure Date: 2025-04-29 | Arrival Date: 2025-04-29 | Marketing Carrier: ANA | Flight Number: NH108 | Cabin Class: Economy",
                        "SIN",
                        "SFO",
                        "2025-04-29",
                        "2025-04-29",
                        "ANA",
                        "NH108",
                        "Economy",
                    ),
                ],
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "1287.44".to_owned(),
                        currency: Some("USD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Fare Total: USD 1287.44",
                )),
            }),
            issues: Vec::new(),
        },
    }
}

fn hotel_folio_guest_bill_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/hotel_folio/hotel_folio_guest_bill.md";
    let filename = "hotel_folio_guest_bill.md";
    let document_id = "hotel_folio_guest_bill";
    CuratedCorpusCase {
        id: "hotel_folio_guest_bill",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(
                DocumentKind::HotelFolio,
                document_id,
                filename,
                "Guest Folio",
            ),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                guest_name: Some(observed(
                    "Olivia Park".to_owned(),
                    document_id,
                    filename,
                    "Guest Olivia Park",
                )),
                property_name: Some(Observed {
                    value: "MARINA BAY GRAND HOTEL".to_owned(),
                    confidence: ConfidenceLevel::Medium,
                    evidence: vec![evidence(document_id, filename, "MARINA BAY GRAND HOTEL")],
                    flags: Vec::new(),
                }),
                folio_number: Some(observed(
                    "MBG-88421".to_owned(),
                    document_id,
                    filename,
                    "Folio # MBG-88421",
                )),
                stay_window: Some(Observed {
                    value: DateRange {
                        start_date: "2025-04-21".to_owned(),
                        end_date: "2025-04-24".to_owned(),
                    },
                    confidence: ConfidenceLevel::Medium,
                    evidence: vec![
                        evidence(document_id, filename, "Check-in 2025-04-21"),
                        evidence(document_id, filename, "Check-out 2025-04-24"),
                        system_evidence("document_extract.infer_stay_window_from_checkin_checkout"),
                    ],
                    flags: Vec::new(),
                }),
                property_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: Some("Singapore".to_owned()),
                        airport_code: None,
                    },
                    document_id,
                    filename,
                    "City/Country Singapore, Singapore",
                )),
                nightly_charges: vec![
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-21  Deluxe King Room  SGD 220.00  SGD 39.60",
                        "2025-04-21",
                    ),
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-22  Deluxe King Room  SGD 220.00  SGD 39.60",
                        "2025-04-22",
                    ),
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-23  Deluxe King Room  SGD 220.00  SGD 39.60",
                        "2025-04-23",
                    ),
                ],
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "778.80".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Balance Due SGD 778.80",
                )),
                meals_included: vec![
                    observed("Breakfast".to_owned(), document_id, filename, "Breakfast"),
                    observed(
                        "Evening Reception".to_owned(),
                        document_id,
                        filename,
                        "Evening Reception",
                    ),
                ],
            }),
            issues: Vec::new(),
        },
    }
}

fn hotel_folio_property_labeled_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/hotel_folio/hotel_folio_property_labeled.md";
    let filename = "hotel_folio_property_labeled.md";
    let document_id = "hotel_folio_property_labeled";
    CuratedCorpusCase {
        id: "hotel_folio_property_labeled",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(
                DocumentKind::HotelFolio,
                document_id,
                filename,
                "Hotel Folio",
            ),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                guest_name: Some(observed(
                    "Olivia Park".to_owned(),
                    document_id,
                    filename,
                    "Guest Name: Olivia Park",
                )),
                property_name: Some(observed(
                    "Marina Bay Grand Hotel".to_owned(),
                    document_id,
                    filename,
                    "Property Name: Marina Bay Grand Hotel",
                )),
                folio_number: Some(observed(
                    "MBG-88421".to_owned(),
                    document_id,
                    filename,
                    "Folio Number: MBG-88421",
                )),
                stay_window: Some(observed(
                    DateRange {
                        start_date: "2025-04-21".to_owned(),
                        end_date: "2025-04-24".to_owned(),
                    },
                    document_id,
                    filename,
                    "Stay Window: 2025-04-21 to 2025-04-24",
                )),
                property_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: Some("Singapore".to_owned()),
                        airport_code: None,
                    },
                    document_id,
                    filename,
                    "Property Location: Singapore, Singapore",
                )),
                nightly_charges: vec![
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-21 | Deluxe King Room | SGD 220.00 | SGD 39.60",
                        "2025-04-21",
                    ),
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-22 | Deluxe King Room | SGD 220.00 | SGD 39.60",
                        "2025-04-22",
                    ),
                    hotel_night(
                        document_id,
                        filename,
                        "2025-04-23 | Deluxe King Room | SGD 220.00 | SGD 39.60",
                        "2025-04-23",
                    ),
                ],
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "778.80".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Total Paid: SGD 778.80",
                )),
                meals_included: vec![
                    observed("Breakfast".to_owned(), document_id, filename, "Breakfast"),
                    observed(
                        "Evening Reception".to_owned(),
                        document_id,
                        filename,
                        "Evening Reception",
                    ),
                ],
            }),
            issues: Vec::new(),
        },
    }
}

fn receipt_card_dotted_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/receipt/receipt_card_dotted.md";
    let filename = "receipt_card_dotted.md";
    let document_id = "receipt_card_dotted";
    CuratedCorpusCase {
        id: "receipt_card_dotted",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(DocumentKind::Receipt, document_id, filename, "Card Receipt"),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(Observed {
                    value: "EAST BAY BISTRO".to_owned(),
                    confidence: ConfidenceLevel::Medium,
                    evidence: vec![evidence(document_id, filename, "EAST BAY BISTRO")],
                    flags: Vec::new(),
                }),
                merchant_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: Some("Singapore".to_owned()),
                        airport_code: None,
                    },
                    document_id,
                    filename,
                    "Location Singapore, Singapore",
                )),
                transaction_date: Some(observed(
                    "2025-04-24".to_owned(),
                    document_id,
                    filename,
                    "Date 2025-04-24",
                )),
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "35.02".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Total SGD 35.02",
                )),
                subtotal: Some(observed(
                    MoneyAmount {
                        amount: "28.00".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Subtotal SGD 28.00",
                )),
                tax_amount: Some(observed(
                    MoneyAmount {
                        amount: "2.52".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "GST SGD 2.52",
                )),
                tip_amount: Some(observed(
                    MoneyAmount {
                        amount: "4.50".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Tip SGD 4.50",
                )),
                line_items: vec![
                    receipt_item(document_id, filename, "Laksa Lunch ........ SGD 18.00", "Laksa Lunch", "18.00"),
                    receipt_item(document_id, filename, "Iced Tea ........... SGD 6.00", "Iced Tea", "6.00"),
                    receipt_item(
                        document_id,
                        filename,
                        "Service Charge ..... SGD 4.00",
                        "Service Charge",
                        "4.00",
                    ),
                ],
            }),
            issues: Vec::new(),
        },
    }
}

fn receipt_merchant_labeled_case() -> CuratedCorpusCase {
    let relative_path = "fixtures/curated/receipt/receipt_merchant_labeled.md";
    let filename = "receipt_merchant_labeled.md";
    let document_id = "receipt_merchant_labeled";
    CuratedCorpusCase {
        id: "receipt_merchant_labeled",
        relative_path,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.to_owned(),
            filename: filename.to_owned(),
            classification: classification(
                DocumentKind::Receipt,
                document_id,
                filename,
                "Merchant Receipt",
            ),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(observed(
                    "East Bay Bistro".to_owned(),
                    document_id,
                    filename,
                    "Merchant: East Bay Bistro",
                )),
                merchant_location: Some(observed(
                    Location {
                        city: Some("Singapore".to_owned()),
                        region: None,
                        country: Some("Singapore".to_owned()),
                        airport_code: None,
                    },
                    document_id,
                    filename,
                    "Merchant Location: Singapore, Singapore",
                )),
                transaction_date: Some(observed(
                    "2025-04-24".to_owned(),
                    document_id,
                    filename,
                    "Transaction Date: 2025-04-24",
                )),
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: "35.02".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Amount Paid: SGD 35.02",
                )),
                subtotal: Some(observed(
                    MoneyAmount {
                        amount: "28.00".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Subtotal: SGD 28.00",
                )),
                tax_amount: Some(observed(
                    MoneyAmount {
                        amount: "2.52".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Tax: SGD 2.52",
                )),
                tip_amount: Some(observed(
                    MoneyAmount {
                        amount: "4.50".to_owned(),
                        currency: Some("SGD".to_owned()),
                    },
                    document_id,
                    filename,
                    "Tip: SGD 4.50",
                )),
                line_items: vec![
                    receipt_item(document_id, filename, "Laksa Lunch    SGD 18.00", "Laksa Lunch", "18.00"),
                    receipt_item(document_id, filename, "Iced Tea       SGD 6.00", "Iced Tea", "6.00"),
                    receipt_item(
                        document_id,
                        filename,
                        "Service Charge SGD 4.00",
                        "Service Charge",
                        "4.00",
                    ),
                ],
            }),
            issues: Vec::new(),
        },
    }
}

fn classification(kind: DocumentKind, document_id: &str, filename: &str, quote: &str) -> DocumentClassification {
    DocumentClassification {
        kind,
        confidence: ConfidenceLevel::High,
        evidence: vec![evidence(document_id, filename, quote)],
        flags: Vec::new(),
    }
}

fn observed<T>(value: T, document_id: &str, filename: &str, quote: &str) -> Observed<T> {
    Observed {
        value,
        confidence: ConfidenceLevel::High,
        evidence: vec![evidence(document_id, filename, quote)],
        flags: Vec::new(),
    }
}

fn evidence(document_id: &str, filename: &str, quote: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::DocumentSpan,
        document_id: Some(document_id.to_owned()),
        filename: Some(filename.to_owned()),
        page: Some(1),
        quote: Some(quote.to_owned()),
        origin: None,
    }
}

fn system_evidence(origin: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::SystemGenerated,
        document_id: None,
        filename: None,
        page: None,
        quote: None,
        origin: Some(origin.to_owned()),
    }
}

fn flight_segment(
    document_id: &str,
    filename: &str,
    quote: &str,
    departure_airport: &str,
    arrival_airport: &str,
    departure_date: &str,
    arrival_date: &str,
    marketing_carrier: &str,
    flight_number: &str,
    cabin_class: &str,
) -> FlightSegmentFacts {
    FlightSegmentFacts {
        departure_airport: observed(departure_airport.to_owned(), document_id, filename, quote),
        arrival_airport: observed(arrival_airport.to_owned(), document_id, filename, quote),
        departure_date: observed(departure_date.to_owned(), document_id, filename, quote),
        arrival_date: Some(observed(arrival_date.to_owned(), document_id, filename, quote)),
        marketing_carrier: Some(observed(marketing_carrier.to_owned(), document_id, filename, quote)),
        flight_number: Some(observed(flight_number.to_owned(), document_id, filename, quote)),
        cabin_class: Some(observed(cabin_class.to_owned(), document_id, filename, quote)),
    }
}

fn hotel_night(document_id: &str, filename: &str, quote: &str, date: &str) -> HotelNightChargeFacts {
    HotelNightChargeFacts {
        date: observed(date.to_owned(), document_id, filename, quote),
        room_rate: Some(observed(
            MoneyAmount {
                amount: "220.00".to_owned(),
                currency: Some("SGD".to_owned()),
            },
            document_id,
            filename,
            quote,
        )),
        taxes_and_fees: vec![observed(
            MoneyAmount {
                amount: "39.60".to_owned(),
                currency: Some("SGD".to_owned()),
            },
            document_id,
            filename,
            quote,
        )],
        description: Some(observed(
            "Deluxe King Room".to_owned(),
            document_id,
            filename,
            quote,
        )),
    }
}

fn receipt_item(
    document_id: &str,
    filename: &str,
    quote: &str,
    description: &str,
    amount: &str,
) -> ReceiptLineItemFacts {
    ReceiptLineItemFacts {
        description: observed(description.to_owned(), document_id, filename, quote),
        amount: observed(
            MoneyAmount {
                amount: amount.to_owned(),
                currency: Some("SGD".to_owned()),
            },
            document_id,
            filename,
            quote,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_facts::parse_document_facts_json_str;

    #[test]
    fn curated_corpus_verifies_cleanly() {
        let report = verify_curated_corpus();
        assert!(
            report.is_clean(),
            "curated corpus failures: {:?}",
            report.failures
        );
    }

    #[test]
    fn curated_expected_facts_round_trip_through_json() {
        for case in curated_corpus_cases() {
            let rendered = render_document_facts_json_pretty(&case.expected_facts)
                .expect("expected facts should render");
            let reparsed = parse_document_facts_json_str(&rendered)
                .expect("rendered expected facts should parse");
            assert_eq!(reparsed, case.expected_facts);
        }
    }
}
