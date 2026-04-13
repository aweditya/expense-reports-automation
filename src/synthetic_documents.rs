use crate::document_facts::{
    DateRange, DocumentClassification, DocumentFactsPayload, DocumentKind, ExtractedDocumentFacts,
    ExtractionStatus, FlightItineraryFacts, FlightSegmentFacts, HotelFolioFacts,
    HotelNightChargeFacts, Location, MoneyAmount, Observed, ReceiptFacts, ReceiptLineItemFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticVariant {
    Baseline,
    Noisy,
}

impl SyntheticVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Noisy => "noisy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticDocumentFixture {
    pub kind: DocumentKind,
    pub filename: String,
    pub markdown: String,
    pub expected_facts: ExtractedDocumentFacts,
}

pub fn generate_synthetic_packet(variant: SyntheticVariant) -> Vec<SyntheticDocumentFixture> {
    vec![
        generate_synthetic_document(DocumentKind::FlightItinerary, variant),
        generate_synthetic_document(DocumentKind::HotelFolio, variant),
        generate_synthetic_document(DocumentKind::Receipt, variant),
    ]
}

pub fn generate_synthetic_document(
    kind: DocumentKind,
    variant: SyntheticVariant,
) -> SyntheticDocumentFixture {
    match kind {
        DocumentKind::FlightItinerary => synthetic_flight_itinerary(variant),
        DocumentKind::HotelFolio => synthetic_hotel_folio(variant),
        DocumentKind::Receipt => synthetic_receipt(variant),
        _ => panic!(
            "synthetic generator currently supports flight itinerary, hotel folio, and receipt"
        ),
    }
}

fn synthetic_flight_itinerary(variant: SyntheticVariant) -> SyntheticDocumentFixture {
    let filename = format!("synthetic_flight_itinerary_{}.md", variant.as_str());
    let document_id = document_id_for_filename(&filename);
    let heading = match variant {
        SyntheticVariant::Baseline => "# E-Ticket Itinerary / Receipt",
        SyntheticVariant::Noisy => "## e-ticket itinerary / receipt",
    };
    let bullet = match variant {
        SyntheticVariant::Baseline => "-",
        SyntheticVariant::Noisy => "*",
    };
    let passenger_heading = match variant {
        SyntheticVariant::Baseline => "## Passenger",
        SyntheticVariant::Noisy => "### passenger",
    };
    let trip_heading = match variant {
        SyntheticVariant::Baseline => "## Trip Summary",
        SyntheticVariant::Noisy => "### trip summary",
    };
    let segment_heading = match variant {
        SyntheticVariant::Baseline => "## Segments",
        SyntheticVariant::Noisy => "### segments",
    };

    let traveler_line = format!("{bullet} Traveler Name: Olivia Park");
    let booking_reference_line = format!("{bullet} Booking Reference: H7K9Q2");
    let ticket_number_line = format!("{bullet} Ticket Number: 0162459135784");
    let booking_date_line = format!("{bullet} Booking Date: 2025-04-10");
    let origin_line = format!("{bullet} Origin: San Francisco, CA, United States (SFO)");
    let destination_line = format!("{bullet} Destination: Singapore (SIN)");
    let trip_window_line = format!("{bullet} Trip Window: 2025-04-21 to 2025-04-29");
    let total_paid_line = format!("{bullet} Total Paid: USD 1287.44");
    let segment_one_line = format!("{bullet} Segment 1 | Departure Airport: SFO | Arrival Airport: NRT | Departure Date: 2025-04-21 | Arrival Date: 2025-04-22 | Marketing Carrier: ANA | Flight Number: NH107 | Cabin Class: Economy");
    let segment_two_line = format!("{bullet} Segment 2 | Departure Airport: SIN | Arrival Airport: SFO | Departure Date: 2025-04-29 | Arrival Date: 2025-04-29 | Marketing Carrier: ANA | Flight Number: NH108 | Cabin Class: Economy");

    let markdown = [
        heading,
        "",
        passenger_heading,
        &traveler_line,
        &booking_reference_line,
        &ticket_number_line,
        &booking_date_line,
        "",
        trip_heading,
        &origin_line,
        &destination_line,
        &trip_window_line,
        &total_paid_line,
        "",
        segment_heading,
        &segment_one_line,
        &segment_two_line,
        "",
    ]
    .join("\n");

    let expected_facts = ExtractedDocumentFacts {
        document_id: document_id.clone(),
        filename: filename.clone(),
        classification: classification(
            DocumentKind::FlightItinerary,
            &document_id,
            &filename,
            heading,
        ),
        extraction_status: ExtractionStatus::Complete,
        facts: DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
            traveler_names: vec![observed(
                "Olivia Park".to_owned(),
                &document_id,
                &filename,
                &traveler_line,
            )],
            confirmation_code: Some(observed(
                "H7K9Q2".to_owned(),
                &document_id,
                &filename,
                &booking_reference_line,
            )),
            ticket_number: Some(observed(
                "0162459135784".to_owned(),
                &document_id,
                &filename,
                &ticket_number_line,
            )),
            booking_date: Some(observed(
                "2025-04-10".to_owned(),
                &document_id,
                &filename,
                &booking_date_line,
            )),
            trip_window: Some(observed(
                DateRange {
                    start_date: "2025-04-21".to_owned(),
                    end_date: "2025-04-29".to_owned(),
                },
                &document_id,
                &filename,
                &trip_window_line,
            )),
            departure_location: Some(observed(
                Location {
                    city: Some("San Francisco".to_owned()),
                    region: Some("CA".to_owned()),
                    country: Some("United States".to_owned()),
                    airport_code: Some("SFO".to_owned()),
                },
                &document_id,
                &filename,
                &origin_line,
            )),
            arrival_location: Some(observed(
                Location {
                    city: Some("Singapore".to_owned()),
                    region: None,
                    country: None,
                    airport_code: Some("SIN".to_owned()),
                },
                &document_id,
                &filename,
                &destination_line,
            )),
            segments: vec![
                FlightSegmentFacts {
                    departure_airport: observed(
                        "SFO".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    ),
                    arrival_airport: observed(
                        "NRT".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    ),
                    departure_date: observed(
                        "2025-04-21".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    ),
                    arrival_date: Some(observed(
                        "2025-04-22".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    )),
                    marketing_carrier: Some(observed(
                        "ANA".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    )),
                    flight_number: Some(observed(
                        "NH107".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    )),
                    cabin_class: Some(observed(
                        "Economy".to_owned(),
                        &document_id,
                        &filename,
                        &segment_one_line,
                    )),
                },
                FlightSegmentFacts {
                    departure_airport: observed(
                        "SIN".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    ),
                    arrival_airport: observed(
                        "SFO".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    ),
                    departure_date: observed(
                        "2025-04-29".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    ),
                    arrival_date: Some(observed(
                        "2025-04-29".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    )),
                    marketing_carrier: Some(observed(
                        "ANA".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    )),
                    flight_number: Some(observed(
                        "NH108".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    )),
                    cabin_class: Some(observed(
                        "Economy".to_owned(),
                        &document_id,
                        &filename,
                        &segment_two_line,
                    )),
                },
            ],
            total_paid: Some(observed(
                MoneyAmount {
                    amount: "1287.44".to_owned(),
                    currency: Some("USD".to_owned()),
                },
                &document_id,
                &filename,
                &total_paid_line,
            )),
        }),
        issues: Vec::new(),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::FlightItinerary,
        filename,
        markdown,
        expected_facts,
    }
}

fn synthetic_hotel_folio(variant: SyntheticVariant) -> SyntheticDocumentFixture {
    let filename = format!("synthetic_hotel_folio_{}.md", variant.as_str());
    let document_id = document_id_for_filename(&filename);
    let heading = match variant {
        SyntheticVariant::Baseline => "# Hotel Folio",
        SyntheticVariant::Noisy => "## guest folio",
    };
    let bullet = match variant {
        SyntheticVariant::Baseline => "-",
        SyntheticVariant::Noisy => "*",
    };
    let stay_heading = match variant {
        SyntheticVariant::Baseline => "## Stay Summary",
        SyntheticVariant::Noisy => "### stay summary",
    };
    let nightly_heading = match variant {
        SyntheticVariant::Baseline => "## Nightly Charges",
        SyntheticVariant::Noisy => "### nightly charges",
    };
    let meals_heading = match variant {
        SyntheticVariant::Baseline => "## Meals Included",
        SyntheticVariant::Noisy => "### meals included",
    };

    let property_line = format!("{bullet} Property Name: Marina Bay Grand Hotel");
    let guest_line = format!("{bullet} Guest Name: Olivia Park");
    let folio_line = format!("{bullet} Folio Number: MBG-88421");
    let location_line = format!("{bullet} Property Location: Singapore, Singapore");
    let stay_window_line = format!("{bullet} Stay Window: 2025-04-21 to 2025-04-24");
    let total_line = format!("{bullet} Total Paid: SGD 778.80");
    let night_one_line = format!("{bullet} Date: 2025-04-21 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60");
    let night_two_line = format!("{bullet} Date: 2025-04-22 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60");
    let night_three_line = format!("{bullet} Date: 2025-04-23 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60");
    let meal_one_line = format!("{bullet} Breakfast");
    let meal_two_line = format!("{bullet} Evening Reception");

    let markdown = [
        heading,
        "",
        stay_heading,
        &property_line,
        &guest_line,
        &folio_line,
        &location_line,
        &stay_window_line,
        &total_line,
        "",
        nightly_heading,
        &night_one_line,
        &night_two_line,
        &night_three_line,
        "",
        meals_heading,
        &meal_one_line,
        &meal_two_line,
        "",
    ]
    .join("\n");

    let expected_facts = ExtractedDocumentFacts {
        document_id: document_id.clone(),
        filename: filename.clone(),
        classification: classification(DocumentKind::HotelFolio, &document_id, &filename, heading),
        extraction_status: ExtractionStatus::Complete,
        facts: DocumentFactsPayload::HotelFolio(HotelFolioFacts {
            guest_name: Some(observed(
                "Olivia Park".to_owned(),
                &document_id,
                &filename,
                &guest_line,
            )),
            property_name: Some(observed(
                "Marina Bay Grand Hotel".to_owned(),
                &document_id,
                &filename,
                &property_line,
            )),
            folio_number: Some(observed(
                "MBG-88421".to_owned(),
                &document_id,
                &filename,
                &folio_line,
            )),
            stay_window: Some(observed(
                DateRange {
                    start_date: "2025-04-21".to_owned(),
                    end_date: "2025-04-24".to_owned(),
                },
                &document_id,
                &filename,
                &stay_window_line,
            )),
            property_location: Some(observed(
                Location {
                    city: Some("Singapore".to_owned()),
                    region: None,
                    country: Some("Singapore".to_owned()),
                    airport_code: None,
                },
                &document_id,
                &filename,
                &location_line,
            )),
            nightly_charges: vec![
                nightly_charge(&document_id, &filename, &night_one_line, "2025-04-21"),
                nightly_charge(&document_id, &filename, &night_two_line, "2025-04-22"),
                nightly_charge(&document_id, &filename, &night_three_line, "2025-04-23"),
            ],
            total_paid: Some(observed(
                MoneyAmount {
                    amount: "778.80".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
                &document_id,
                &filename,
                &total_line,
            )),
            meals_included: vec![
                observed(
                    "Breakfast".to_owned(),
                    &document_id,
                    &filename,
                    &meal_one_line,
                ),
                observed(
                    "Evening Reception".to_owned(),
                    &document_id,
                    &filename,
                    &meal_two_line,
                ),
            ],
        }),
        issues: Vec::new(),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::HotelFolio,
        filename,
        markdown,
        expected_facts,
    }
}

fn synthetic_receipt(variant: SyntheticVariant) -> SyntheticDocumentFixture {
    let filename = format!("synthetic_receipt_{}.md", variant.as_str());
    let document_id = document_id_for_filename(&filename);
    let heading = match variant {
        SyntheticVariant::Baseline => "# Merchant Receipt",
        SyntheticVariant::Noisy => "## card receipt",
    };
    let bullet = match variant {
        SyntheticVariant::Baseline => "-",
        SyntheticVariant::Noisy => "*",
    };
    let purchase_heading = match variant {
        SyntheticVariant::Baseline => "## Purchase Summary",
        SyntheticVariant::Noisy => "### purchase summary",
    };
    let line_items_heading = match variant {
        SyntheticVariant::Baseline => "## Line Items",
        SyntheticVariant::Noisy => "### line items",
    };

    let merchant_line = format!("{bullet} Merchant Name: East Bay Bistro");
    let location_line = format!("{bullet} Merchant Location: Singapore, Singapore");
    let date_line = format!("{bullet} Transaction Date: 2025-04-24");
    let subtotal_line = format!("{bullet} Subtotal: SGD 28.00");
    let tax_line = format!("{bullet} Tax: SGD 2.52");
    let tip_line = format!("{bullet} Tip: SGD 4.50");
    let total_line = format!("{bullet} Total Paid: SGD 35.02");
    let item_one_line = format!("{bullet} Laksa Lunch | SGD 18.00");
    let item_two_line = format!("{bullet} Iced Tea | SGD 6.00");
    let item_three_line = format!("{bullet} Service Charge | SGD 4.00");

    let markdown = [
        heading,
        "",
        purchase_heading,
        &merchant_line,
        &location_line,
        &date_line,
        &subtotal_line,
        &tax_line,
        &tip_line,
        &total_line,
        "",
        line_items_heading,
        &item_one_line,
        &item_two_line,
        &item_three_line,
        "",
    ]
    .join("\n");

    let expected_facts = ExtractedDocumentFacts {
        document_id: document_id.clone(),
        filename: filename.clone(),
        classification: classification(DocumentKind::Receipt, &document_id, &filename, heading),
        extraction_status: ExtractionStatus::Complete,
        facts: DocumentFactsPayload::Receipt(ReceiptFacts {
            merchant_name: Some(observed(
                "East Bay Bistro".to_owned(),
                &document_id,
                &filename,
                &merchant_line,
            )),
            merchant_location: Some(observed(
                Location {
                    city: Some("Singapore".to_owned()),
                    region: None,
                    country: Some("Singapore".to_owned()),
                    airport_code: None,
                },
                &document_id,
                &filename,
                &location_line,
            )),
            transaction_date: Some(observed(
                "2025-04-24".to_owned(),
                &document_id,
                &filename,
                &date_line,
            )),
            total_paid: Some(observed(
                MoneyAmount {
                    amount: "35.02".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
                &document_id,
                &filename,
                &total_line,
            )),
            subtotal: Some(observed(
                MoneyAmount {
                    amount: "28.00".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
                &document_id,
                &filename,
                &subtotal_line,
            )),
            tax_amount: Some(observed(
                MoneyAmount {
                    amount: "2.52".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
                &document_id,
                &filename,
                &tax_line,
            )),
            tip_amount: Some(observed(
                MoneyAmount {
                    amount: "4.50".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
                &document_id,
                &filename,
                &tip_line,
            )),
            line_items: vec![
                receipt_line_item(
                    &document_id,
                    &filename,
                    &item_one_line,
                    "Laksa Lunch",
                    "18.00",
                ),
                receipt_line_item(&document_id, &filename, &item_two_line, "Iced Tea", "6.00"),
                receipt_line_item(
                    &document_id,
                    &filename,
                    &item_three_line,
                    "Service Charge",
                    "4.00",
                ),
            ],
        }),
        issues: Vec::new(),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::Receipt,
        filename,
        markdown,
        expected_facts,
    }
}

fn nightly_charge(
    document_id: &str,
    filename: &str,
    quote: &str,
    date: &str,
) -> HotelNightChargeFacts {
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

fn receipt_line_item(
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

fn classification(
    kind: DocumentKind,
    document_id: &str,
    filename: &str,
    quote: &str,
) -> DocumentClassification {
    DocumentClassification {
        kind,
        confidence: ConfidenceLevel::High,
        evidence: vec![document_span(document_id, filename, quote)],
        flags: Vec::new(),
    }
}

fn observed<T>(value: T, document_id: &str, filename: &str, quote: &str) -> Observed<T> {
    Observed::new(
        value,
        ConfidenceLevel::High,
        vec![document_span(document_id, filename, quote)],
    )
}

fn document_span(document_id: &str, filename: &str, quote: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::DocumentSpan,
        document_id: Some(document_id.to_owned()),
        filename: Some(filename.to_owned()),
        page: Some(1),
        quote: Some(quote.trim().to_owned()),
        origin: None,
    }
}

fn document_id_for_filename(filename: &str) -> String {
    let stem = filename
        .strip_suffix(".md")
        .or_else(|| filename.strip_suffix(".txt"))
        .unwrap_or(filename);
    let mut identifier = String::new();
    let mut previous_was_separator = false;

    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            identifier.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            identifier.push('_');
            previous_was_separator = true;
        }
    }

    identifier.trim_matches('_').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_three_documents_for_each_packet() {
        let packet = generate_synthetic_packet(SyntheticVariant::Baseline);
        assert_eq!(packet.len(), 3);
        assert_eq!(packet[0].kind, DocumentKind::FlightItinerary);
        assert_eq!(packet[1].kind, DocumentKind::HotelFolio);
        assert_eq!(packet[2].kind, DocumentKind::Receipt);
    }

    #[test]
    fn noisy_variant_uses_markdown_headings_and_expected_kinds() {
        let receipt = generate_synthetic_document(DocumentKind::Receipt, SyntheticVariant::Noisy);
        assert!(receipt.markdown.contains("card receipt"));
        assert_eq!(
            receipt.expected_facts.classification.kind,
            DocumentKind::Receipt
        );
        assert!(receipt.expected_facts.validate_contract().is_ok());
    }
}
