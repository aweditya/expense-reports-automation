use std::path::Path;

use crate::document_facts::{
    DateRange, DocumentClassification, DocumentExtractionIssue, DocumentFactsPayload, DocumentKind,
    ExtractedDocumentFacts, ExtractionStatus, FlightItineraryFacts, FlightSegmentFacts,
    HotelFolioFacts, HotelNightChargeFacts, IssueSeverity, Location, MoneyAmount, Observed,
    ReceiptFacts, ReceiptLineItemFacts, UnknownDocumentFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};
use crate::transcribe::{
    transcribe_document_path, TranscribedDocument, TranscribedPage, TranscribedRegion,
    TranscriptionError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct LineRef {
    page_number: u32,
    raw: String,
    normalized: String,
    is_heading: bool,
    is_bullet: bool,
}

pub fn extract_document_facts_path(
    path: impl AsRef<Path>,
) -> Result<ExtractedDocumentFacts, TranscriptionError> {
    let document = transcribe_document_path(path)?;
    Ok(extract_document_facts(&document))
}

pub fn extract_document_facts(document: &TranscribedDocument) -> ExtractedDocumentFacts {
    let lines = collect_lines(document);
    let classification = classify_document(document, &lines);

    match classification.kind {
        DocumentKind::FlightItinerary => extract_flight_itinerary(document, &lines, classification),
        DocumentKind::HotelFolio => extract_hotel_folio(document, &lines, classification),
        DocumentKind::Receipt => extract_receipt(document, &lines, classification),
        _ => unknown_document(document, classification, &lines),
    }
}

fn collect_lines(document: &TranscribedDocument) -> Vec<LineRef> {
    let mut lines = Vec::new();
    for page in &document.pages {
        for raw_line in page.text.lines() {
            let trimmed: &str = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let is_heading = looks_like_heading(trimmed);
            let is_bullet = trimmed.starts_with('-') || trimmed.starts_with('*');
            lines.push(LineRef {
                page_number: page.page_number,
                raw: trimmed.to_owned(),
                normalized: normalize_line(trimmed),
                is_heading,
                is_bullet,
            });
        }
    }
    lines
}

fn classify_document(document: &TranscribedDocument, lines: &[LineRef]) -> DocumentClassification {
    if let Some(line) = find_line_with_any(
        lines,
        &[
            "itinerary / receipt",
            "itinerary receipt",
            "e-ticket itinerary",
            "e ticket itinerary",
        ],
    ) {
        return classification_from_line(
            document,
            DocumentKind::FlightItinerary,
            line,
            ConfidenceLevel::High,
        );
    }

    if let Some(line) = find_line_with_any(lines, &["hotel folio", "guest folio", "guest bill"]) {
        return classification_from_line(
            document,
            DocumentKind::HotelFolio,
            line,
            ConfidenceLevel::High,
        );
    }

    if let Some(line) = find_line_with_any(lines, &["merchant receipt", "card receipt"]) {
        return classification_from_line(
            document,
            DocumentKind::Receipt,
            line,
            ConfidenceLevel::High,
        );
    }

    if let Some(classification) = fallback_receipt_classification(document, lines) {
        return classification;
    }

    if let Some(classification) = grounded_receipt_classification(document) {
        return classification;
    }

    if let Some(line) = lines.first() {
        let mut classification =
            classification_from_line(document, DocumentKind::Unknown, line, ConfidenceLevel::Low);
        classification
            .flags
            .push("unsupported_document_kind".to_owned());
        classification
    } else {
        DocumentClassification {
            kind: DocumentKind::Unknown,
            confidence: ConfidenceLevel::Low,
            evidence: vec![system_generated_evidence("empty_transcription")],
            flags: vec!["empty_document".to_owned()],
        }
    }
}

fn fallback_receipt_classification(
    document: &TranscribedDocument,
    lines: &[LineRef],
) -> Option<DocumentClassification> {
    let total_line = lines.iter().find(|line| {
        strip_label_value(
            &line.raw,
            &[
                "final total",
                "rounded total",
                "total rounded",
                "grand total",
                "net total",
                "total paid",
                "amount paid",
                "total sales",
                "total",
                "total amt",
            ],
            !line.is_heading,
        )
        .is_some()
    })?;

    let has_supporting_signal = lines.iter().any(|line| {
        strip_label_value(
            &line.raw,
            &["subtotal", "tax", "gst", "vat", "tip"],
            !line.is_heading,
        )
        .is_some()
            || strip_label_value(&line.raw, &["transaction date", "date"], !line.is_heading)
                .is_some()
            || find_pipe_value(line, &["transaction date", "date"]).is_some()
            || strip_label_value(
                &line.raw,
                &["card", "authorization code", "terminal id"],
                !line.is_heading,
            )
            .is_some()
            || find_pipe_value(line, &["card", "authorization code", "terminal id"]).is_some()
            || looks_like_receipt_item_line(line)
    });

    if !has_supporting_signal {
        return None;
    }

    Some(classification_from_line(
        document,
        DocumentKind::Receipt,
        total_line,
        ConfidenceLevel::Medium,
    ))
}

fn grounded_receipt_classification(document: &TranscribedDocument) -> Option<DocumentClassification> {
    let supported_region_count = ["merchant_name", "transaction_date", "total_paid"]
        .iter()
        .filter(|region_id| grounded_region(document, region_id).is_some())
        .count();
    if supported_region_count < 2 {
        return None;
    }

    if let Some((page, region)) = grounded_region(document, "total_paid") {
        return Some(classification_from_region(
            document,
            DocumentKind::Receipt,
            page,
            region,
            ConfidenceLevel::Medium,
            vec!["grounded_receipt_classification".to_owned()],
        ));
    }

    if let Some((page, region)) = grounded_region(document, "merchant_name") {
        return Some(classification_from_region(
            document,
            DocumentKind::Receipt,
            page,
            region,
            ConfidenceLevel::Medium,
            vec!["grounded_receipt_classification".to_owned()],
        ));
    }

    None
}

fn grounded_receipt_merchant_name(document: &TranscribedDocument) -> Option<Observed<String>> {
    let (page, region) = grounded_region(document, "merchant_name")?;
    Some(observed_from_region(
        document,
        page,
        region,
        normalize_bullet_content(&region.text),
        ConfidenceLevel::Medium,
        vec!["grounded_receipt_field".to_owned()],
    ))
}

fn grounded_receipt_transaction_date(document: &TranscribedDocument) -> Option<Observed<String>> {
    let (page, region) = grounded_region(document, "transaction_date")?;
    Some(observed_from_region(
        document,
        page,
        region,
        normalize_receipt_date_value(&region.text),
        ConfidenceLevel::Medium,
        vec!["grounded_receipt_field".to_owned()],
    ))
}

fn grounded_receipt_total_paid(document: &TranscribedDocument) -> Option<Observed<MoneyAmount>> {
    let (page, region) = grounded_region(document, "total_paid")?;
    let currency_hint = grounded_region(document, "total_paid_currency")
        .map(|(_, currency_region)| currency_region.text.clone())
        .or_else(|| {
            if document_contains_hangul(document) {
                Some("KRW".to_owned())
            } else {
                None
            }
        });
    let money = parse_grounded_money(&region.text, currency_hint.as_deref())?;
    let mut flags = vec!["grounded_receipt_field".to_owned()];
    if currency_hint.as_deref() == Some("KRW")
        && grounded_region(document, "total_paid_currency").is_none()
        && document_contains_hangul(document)
    {
        flags.push("hangul_currency_inference".to_owned());
    }
    Some(observed_from_region(
        document,
        page,
        region,
        money,
        ConfidenceLevel::Medium,
        flags,
    ))
}

fn grounded_region<'a>(
    document: &'a TranscribedDocument,
    region_id: &str,
) -> Option<(&'a TranscribedPage, &'a TranscribedRegion)> {
    document.pages.iter().find_map(|page| {
        page.regions
            .iter()
            .find(|region| region.region_id == region_id)
            .map(|region| (page, region))
    })
}

fn parse_grounded_money(value: &str, currency_hint: Option<&str>) -> Option<MoneyAmount> {
    if let Some(currency_hint) = currency_hint {
        return Some(MoneyAmount {
            amount: sanitize_amount(value)?,
            currency: Some(normalize_currency_code(currency_hint)),
        });
    }
    parse_money(value)
}

fn document_contains_hangul(document: &TranscribedDocument) -> bool {
    document.pages.iter().any(|page| {
        page.text
            .chars()
            .any(|ch| matches!(ch as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF))
    })
}

fn extract_flight_itinerary(
    document: &TranscribedDocument,
    lines: &[LineRef],
    classification: DocumentClassification,
) -> ExtractedDocumentFacts {
    let traveler_name = observed_string(
        document,
        lines,
        &["traveler name", "passenger name", "traveler", "passenger"],
        ConfidenceLevel::High,
    );
    let confirmation_code = observed_string(
        document,
        lines,
        &[
            "booking reference",
            "confirmation code",
            "record locator",
            "reservation code",
        ],
        ConfidenceLevel::High,
    );
    let ticket_number = observed_string(
        document,
        lines,
        &["ticket number", "ticket no"],
        ConfidenceLevel::High,
    );
    let booking_date = observed_string(
        document,
        lines,
        &["booking date", "issue date", "issued", "booked"],
        ConfidenceLevel::High,
    );
    let departure_location =
        observed_location(document, lines, &["origin", "from"], ConfidenceLevel::High);
    let arrival_location = observed_location(
        document,
        lines,
        &["destination", "to"],
        ConfidenceLevel::High,
    );
    let explicit_trip_window = observed_date_range(
        document,
        lines,
        &["trip window", "travel window"],
        ConfidenceLevel::High,
    )
    .or_else(|| {
        observed_date_range_from_labels(
            document,
            lines,
            &["departure", "outbound"],
            &["return", "arrival"],
            ConfidenceLevel::Medium,
            "document_extract.infer_trip_window_from_departure_return",
        )
    });
    let total_paid = observed_money(
        document,
        lines,
        &["total paid", "fare paid", "total fare paid", "fare total"],
        ConfidenceLevel::High,
    );
    let segment_lines = collect_section_rows(lines, &["segments", "flight segments"]);
    let segments = segment_lines
        .iter()
        .filter_map(|line| parse_flight_segment(document, line))
        .collect::<Vec<_>>();

    let trip_window = explicit_trip_window.or_else(|| infer_trip_window_from_segments(&segments));
    let mut issues = Vec::new();

    if traveler_name.is_none() {
        issues.push(missing_field_issue(
            "missing_traveler_name",
            "Flight itinerary is missing traveler name",
        ));
    }
    if segments.is_empty() {
        issues.push(missing_field_issue(
            "missing_segments",
            "Flight itinerary is missing segment details",
        ));
    }

    let extraction_status = extraction_status_from_issues(&issues);
    let facts = FlightItineraryFacts {
        traveler_names: traveler_name.into_iter().collect(),
        confirmation_code,
        ticket_number,
        booking_date,
        trip_window,
        departure_location,
        arrival_location,
        segments,
        total_paid,
    };

    finalized_document(
        document,
        classification,
        extraction_status,
        DocumentFactsPayload::FlightItinerary(facts),
        issues,
    )
}

fn extract_hotel_folio(
    document: &TranscribedDocument,
    lines: &[LineRef],
    classification: DocumentClassification,
) -> ExtractedDocumentFacts {
    let guest_name = observed_string(
        document,
        lines,
        &["guest name", "guest"],
        ConfidenceLevel::High,
    );
    let property_name = observed_string(
        document,
        lines,
        &["property name", "hotel name"],
        ConfidenceLevel::High,
    )
    .or_else(|| {
        infer_heading_value(
            document,
            lines,
            &["hotel folio", "guest folio", "guest bill"],
        )
    });
    let folio_number = observed_string(
        document,
        lines,
        &["folio number", "folio #", "folio"],
        ConfidenceLevel::High,
    );
    let property_location = observed_location(
        document,
        lines,
        &[
            "property location",
            "hotel location",
            "city/country",
            "location",
        ],
        ConfidenceLevel::High,
    );
    let explicit_stay_window = observed_date_range(
        document,
        lines,
        &["stay window", "check-in / check-out"],
        ConfidenceLevel::High,
    )
    .or_else(|| {
        observed_date_range_from_labels(
            document,
            lines,
            &["check in", "arrival"],
            &["check out", "departure"],
            ConfidenceLevel::Medium,
            "document_extract.infer_stay_window_from_checkin_checkout",
        )
    });
    let nightly_lines = collect_section_rows(lines, &["nightly charges", "charges"]);
    let nightly_lines = merge_wrapped_pipe_rows(&nightly_lines);
    let nightly_charges = nightly_lines
        .iter()
        .filter_map(|line| parse_hotel_night_charge(document, line))
        .collect::<Vec<_>>();
    let stay_window =
        explicit_stay_window.or_else(|| infer_stay_window_from_nights(&nightly_charges));
    let total_paid = observed_money(
        document,
        lines,
        &["total paid", "amount paid", "balance due", "total amount"],
        ConfidenceLevel::High,
    )
    .or_else(|| infer_hotel_total_from_nights(&nightly_charges));
    let meals_included = collect_section_rows(
        lines,
        &[
            "meals included",
            "included in rate",
            "included in room rate",
        ],
    )
    .into_iter()
    .map(|line| {
        observed_from_line(
            line,
            normalize_bullet_content(&line.raw),
            ConfidenceLevel::High,
            document,
        )
    })
    .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if guest_name.is_none() {
        issues.push(missing_field_issue(
            "missing_guest_name",
            "Hotel folio is missing guest name",
        ));
    }
    if property_name.is_none() {
        issues.push(missing_field_issue(
            "missing_property_name",
            "Hotel folio is missing property name",
        ));
    }
    if nightly_charges.is_empty() {
        issues.push(missing_field_issue(
            "missing_nightly_charges",
            "Hotel folio is missing nightly charges",
        ));
    }

    let extraction_status = extraction_status_from_issues(&issues);
    let facts = HotelFolioFacts {
        guest_name,
        property_name,
        folio_number,
        stay_window,
        property_location,
        nightly_charges,
        total_paid,
        meals_included,
    };

    finalized_document(
        document,
        classification,
        extraction_status,
        DocumentFactsPayload::HotelFolio(facts),
        issues,
    )
}

fn extract_receipt(
    document: &TranscribedDocument,
    lines: &[LineRef],
    classification: DocumentClassification,
) -> ExtractedDocumentFacts {
    let merchant_name = observed_string(
        document,
        lines,
        &["merchant name", "merchant"],
        ConfidenceLevel::High,
    )
    .or_else(|| grounded_receipt_merchant_name(document))
    .or_else(|| infer_receipt_merchant_name(document, lines));
    let merchant_location = observed_location(
        document,
        lines,
        &["merchant location", "location"],
        ConfidenceLevel::High,
    );
    let currency_hint = receipt_currency_hint(lines);
    let transaction_date = observed_receipt_date(
        document,
        lines,
        &["transaction date", "date"],
        ConfidenceLevel::High,
    )
    .or_else(|| grounded_receipt_transaction_date(document));
    let subtotal = observed_money(document, lines, &["subtotal"], ConfidenceLevel::High);
    let tax_amount = observed_money(
        document,
        lines,
        &["tax", "gst", "vat"],
        ConfidenceLevel::High,
    );
    let tip_amount = observed_money(document, lines, &["tip"], ConfidenceLevel::High);
    let subtotal = with_money_currency_hint(
        subtotal,
        currency_hint.as_deref(),
        "document_extract.receipt_currency_hint",
    );
    let tax_amount = with_money_currency_hint(
        tax_amount,
        currency_hint.as_deref(),
        "document_extract.receipt_currency_hint",
    );
    let tip_amount = with_money_currency_hint(
        tip_amount,
        currency_hint.as_deref(),
        "document_extract.receipt_currency_hint",
    );
    let total_paid = with_money_currency_hint(
        observed_receipt_total(document, lines, ConfidenceLevel::High)
            .or_else(|| infer_receipt_total(subtotal.as_ref(), tax_amount.as_ref(), tip_amount.as_ref()))
            .or_else(|| grounded_receipt_total_paid(document)),
        currency_hint.as_deref(),
        "document_extract.receipt_currency_hint",
    );
    let section_rows = collect_section_rows(
        lines,
        &["line items", "items", "purchased items", "items purchased"],
    );
    let line_items = if section_rows.is_empty() {
        lines
            .iter()
            .filter(|line| looks_like_receipt_item_line(line))
            .filter_map(|line| parse_receipt_line_item(document, line))
            .collect::<Vec<_>>()
    } else {
        section_rows
            .into_iter()
            .filter_map(|line| parse_receipt_line_item(document, line))
            .collect::<Vec<_>>()
    };

    let mut issues = Vec::new();
    if merchant_name.is_none() {
        issues.push(missing_field_issue(
            "missing_merchant_name",
            "Receipt is missing merchant name",
        ));
    }
    if transaction_date.is_none() {
        issues.push(missing_field_issue(
            "missing_transaction_date",
            "Receipt is missing transaction date",
        ));
    }
    if total_paid.is_none() {
        issues.push(missing_field_issue(
            "missing_total_paid",
            "Receipt is missing total paid",
        ));
    }

    let extraction_status = extraction_status_from_issues(&issues);
    let facts = ReceiptFacts {
        merchant_name,
        merchant_location,
        transaction_date,
        total_paid,
        subtotal,
        tax_amount,
        tip_amount,
        line_items,
    };

    finalized_document(
        document,
        classification,
        extraction_status,
        DocumentFactsPayload::Receipt(facts),
        issues,
    )
}

fn unknown_document(
    document: &TranscribedDocument,
    classification: DocumentClassification,
    lines: &[LineRef],
) -> ExtractedDocumentFacts {
    let title_hint = lines.iter().find(|line| line.is_heading).map(|line| {
        observed_from_line(
            line,
            normalize_bullet_content(&line.raw),
            ConfidenceLevel::Low,
            document,
        )
    });
    let text_summary = lines.first().map(|line| {
        observed_from_line(
            line,
            normalize_bullet_content(&line.raw),
            ConfidenceLevel::Low,
            document,
        )
    });

    finalized_document(
        document,
        classification,
        ExtractionStatus::Unsupported,
        DocumentFactsPayload::Unknown(UnknownDocumentFacts {
            title_hint,
            text_summary,
        }),
        vec![DocumentExtractionIssue {
            severity: IssueSeverity::Warning,
            code: "unsupported_document_kind".to_owned(),
            message: "No extractor is available for this document kind".to_owned(),
            evidence: vec![system_generated_evidence(
                "document_extract.unknown_document",
            )],
        }],
    )
}

fn finalized_document(
    document: &TranscribedDocument,
    classification: DocumentClassification,
    extraction_status: ExtractionStatus,
    facts: DocumentFactsPayload,
    issues: Vec<DocumentExtractionIssue>,
) -> ExtractedDocumentFacts {
    ExtractedDocumentFacts {
        document_id: document.document_id.clone(),
        filename: document.filename.clone(),
        classification,
        extraction_status,
        facts,
        issues,
    }
}

fn extraction_status_from_issues(issues: &[DocumentExtractionIssue]) -> ExtractionStatus {
    if issues
        .iter()
        .any(|issue| issue.severity == IssueSeverity::Error)
    {
        ExtractionStatus::Partial
    } else {
        ExtractionStatus::Complete
    }
}

fn parse_flight_segment(
    document: &TranscribedDocument,
    line: &LineRef,
) -> Option<FlightSegmentFacts> {
    if let Some(departure_airport) = observed_pipe_field(
        document,
        line,
        &["departure airport"],
        ConfidenceLevel::High,
    ) {
        let arrival_airport =
            observed_pipe_field(document, line, &["arrival airport"], ConfidenceLevel::High)?;
        let departure_date =
            observed_pipe_field(document, line, &["departure date"], ConfidenceLevel::High)?;
        let arrival_date =
            observed_pipe_field(document, line, &["arrival date"], ConfidenceLevel::High);
        let marketing_carrier = observed_pipe_field(
            document,
            line,
            &["marketing carrier", "carrier"],
            ConfidenceLevel::High,
        );
        let flight_number =
            observed_pipe_field(document, line, &["flight number"], ConfidenceLevel::High);
        let cabin_class =
            observed_pipe_field(document, line, &["cabin class"], ConfidenceLevel::High);

        return Some(FlightSegmentFacts {
            departure_airport,
            arrival_airport,
            departure_date,
            arrival_date,
            marketing_carrier,
            flight_number,
            cabin_class,
        });
    }

    let columns = split_columns(&normalize_bullet_content(&line.raw));
    if columns.len() < 4 {
        return None;
    }

    let route = columns.iter().find(|column| column.contains("->"))?;
    let dates = columns
        .iter()
        .find(|column| column.contains("->") && column.contains("202"))
        .or_else(|| columns.iter().find(|column| column.contains("202")))?;
    let route_parts = route.split("->").map(str::trim).collect::<Vec<_>>();
    if route_parts.len() != 2 {
        return None;
    }

    let date_parts = dates.split("->").map(str::trim).collect::<Vec<_>>();
    let departure_date = date_parts.first()?.to_string();
    let arrival_date = date_parts.get(1).map(|value| value.to_string());
    let airline_and_flight = columns.iter().find(|column| {
        column.split_whitespace().count() >= 2
            && column.chars().any(|ch| ch.is_ascii_digit())
            && column.chars().any(|ch| ch.is_ascii_alphabetic())
            && !column.contains("->")
    })?;
    let mut airline_tokens = airline_and_flight.split_whitespace();
    let marketing_carrier = airline_tokens.next()?.to_owned();
    let flight_number = airline_tokens.next()?.to_owned();
    let cabin_class = columns.last().map(|value| value.trim().to_owned());

    Some(FlightSegmentFacts {
        departure_airport: observed_from_line(
            line,
            route_parts[0].to_owned(),
            ConfidenceLevel::High,
            document,
        ),
        arrival_airport: observed_from_line(
            line,
            route_parts[1].to_owned(),
            ConfidenceLevel::High,
            document,
        ),
        departure_date: observed_from_line(line, departure_date, ConfidenceLevel::High, document),
        arrival_date: arrival_date
            .map(|value| observed_from_line(line, value, ConfidenceLevel::High, document)),
        marketing_carrier: Some(observed_from_line(
            line,
            marketing_carrier,
            ConfidenceLevel::High,
            document,
        )),
        flight_number: Some(observed_from_line(
            line,
            flight_number,
            ConfidenceLevel::High,
            document,
        )),
        cabin_class: cabin_class
            .map(|value| observed_from_line(line, value, ConfidenceLevel::High, document)),
    })
}

fn parse_hotel_night_charge(
    document: &TranscribedDocument,
    line: &LineRef,
) -> Option<HotelNightChargeFacts> {
    if let Some(date) = observed_pipe_field(document, line, &["date"], ConfidenceLevel::High) {
        let description =
            observed_pipe_field(document, line, &["description"], ConfidenceLevel::High);
        let room_rate =
            observed_pipe_field_money(document, line, &["room rate"], ConfidenceLevel::High);
        let taxes_and_fees =
            observed_pipe_field_money(document, line, &["taxes & fees"], ConfidenceLevel::High)
                .into_iter()
                .collect::<Vec<_>>();

        return Some(HotelNightChargeFacts {
            date,
            room_rate,
            taxes_and_fees,
            description,
        });
    }

    let columns = split_columns(&normalize_bullet_content(&line.raw));
    if columns.len() < 4 {
        return None;
    }

    let date = columns.first()?.to_owned();
    let description = columns.get(1)?.to_owned();
    let room_rate = parse_money(columns.get(2)?)?;
    let taxes_and_fees = parse_money(columns.get(3)?)?;

    Some(HotelNightChargeFacts {
        date: observed_from_line(line, date, ConfidenceLevel::High, document),
        room_rate: Some(observed_from_line(
            line,
            room_rate,
            ConfidenceLevel::High,
            document,
        )),
        taxes_and_fees: vec![observed_from_line(
            line,
            taxes_and_fees,
            ConfidenceLevel::High,
            document,
        )],
        description: Some(observed_from_line(
            line,
            description,
            ConfidenceLevel::High,
            document,
        )),
    })
}

fn parse_receipt_line_item(
    document: &TranscribedDocument,
    line: &LineRef,
) -> Option<ReceiptLineItemFacts> {
    let content = normalize_bullet_content(&line.raw);
    let (description, amount) = if content.contains('|') {
        let mut parts = content.split('|').map(str::trim);
        let description = parts.next()?.to_owned();
        let amount = parse_money(parts.next()?)?;
        (description, amount)
    } else {
        split_description_and_money(&content)?
    };

    let description_key = normalize_key(&description);
    if is_receipt_summary_label(&description_key) {
        return None;
    }

    Some(ReceiptLineItemFacts {
        description: observed_from_line(line, description, ConfidenceLevel::High, document),
        amount: observed_from_line(line, amount, ConfidenceLevel::High, document),
    })
}

fn looks_like_receipt_item_line(line: &LineRef) -> bool {
    let content = normalize_bullet_content(&line.raw);
    if content.contains(':') && !content.contains('|') {
        return false;
    }
    let description = if content.contains('|') {
        let mut parts = content.split('|').map(str::trim);
        let description = match parts.next() {
            Some(value) => value,
            None => return false,
        };
        let amount = match parts.next() {
            Some(value) => value,
            None => return false,
        };
        if parse_money(amount).is_none() {
            return false;
        }
        description.to_owned()
    } else {
        match split_description_and_money(&content) {
            Some((description, _)) => description,
            None => return false,
        }
    };

    !is_receipt_summary_label(&normalize_key(&description))
}

fn is_receipt_summary_label(description_key: &str) -> bool {
    [
        "subtotal",
        "tax",
        "gst",
        "vat",
        "tip",
        "total",
        "total paid",
        "amount paid",
    ]
    .iter()
    .any(|label| description_key == normalize_key(label))
}

fn infer_trip_window_from_segments(segments: &[FlightSegmentFacts]) -> Option<Observed<DateRange>> {
    let start_date = segments.first()?.departure_date.value.clone();
    let end_date = segments
        .last()?
        .arrival_date
        .as_ref()
        .map(|value| value.value.clone())
        .unwrap_or_else(|| segments.last().unwrap().departure_date.value.clone());

    let mut evidence = Vec::new();
    evidence.extend(segments.first()?.departure_date.evidence.clone());
    if let Some(arrival_date) = &segments.last()?.arrival_date {
        evidence.extend(arrival_date.evidence.clone());
    } else {
        evidence.extend(segments.last()?.departure_date.evidence.clone());
    }
    evidence.push(system_generated_evidence(
        "document_extract.infer_trip_window_from_segments",
    ));

    Some(Observed {
        value: DateRange {
            start_date,
            end_date,
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    })
}

fn infer_stay_window_from_nights(nights: &[HotelNightChargeFacts]) -> Option<Observed<DateRange>> {
    let start_date = nights.first()?.date.value.clone();
    let end_date = nights.last()?.date.value.clone();
    let mut evidence = Vec::new();
    evidence.extend(nights.first()?.date.evidence.clone());
    evidence.extend(nights.last()?.date.evidence.clone());
    evidence.push(system_generated_evidence(
        "document_extract.infer_stay_window_from_nights",
    ));

    Some(Observed {
        value: DateRange {
            start_date,
            end_date,
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    })
}

fn infer_hotel_total_from_nights(
    nights: &[HotelNightChargeFacts],
) -> Option<Observed<MoneyAmount>> {
    let mut currency = None;
    let mut total_cents = 0i64;
    let mut evidence = Vec::new();

    for night in nights {
        if let Some(room_rate) = &night.room_rate {
            total_cents += amount_to_cents(&room_rate.value.amount)?;
            currency = currency.or_else(|| room_rate.value.currency.clone());
            evidence.extend(room_rate.evidence.clone());
        }

        for tax in &night.taxes_and_fees {
            total_cents += amount_to_cents(&tax.value.amount)?;
            currency = currency.or_else(|| tax.value.currency.clone());
            evidence.extend(tax.evidence.clone());
        }
    }

    evidence.push(system_generated_evidence(
        "document_extract.infer_hotel_total_from_nights",
    ));

    Some(Observed {
        value: MoneyAmount {
            amount: cents_to_amount(total_cents),
            currency,
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    })
}

fn infer_receipt_total(
    subtotal: Option<&Observed<MoneyAmount>>,
    tax_amount: Option<&Observed<MoneyAmount>>,
    tip_amount: Option<&Observed<MoneyAmount>>,
) -> Option<Observed<MoneyAmount>> {
    let subtotal = subtotal?;
    let mut total_cents = amount_to_cents(&subtotal.value.amount)?;
    let mut evidence = subtotal.evidence.clone();
    let mut currency = subtotal.value.currency.clone();

    if let Some(tax_amount) = tax_amount {
        total_cents += amount_to_cents(&tax_amount.value.amount)?;
        currency = currency.or_else(|| tax_amount.value.currency.clone());
        evidence.extend(tax_amount.evidence.clone());
    }

    if let Some(tip_amount) = tip_amount {
        total_cents += amount_to_cents(&tip_amount.value.amount)?;
        currency = currency.or_else(|| tip_amount.value.currency.clone());
        evidence.extend(tip_amount.evidence.clone());
    }

    evidence.push(system_generated_evidence(
        "document_extract.infer_receipt_total",
    ));

    Some(Observed {
        value: MoneyAmount {
            amount: cents_to_amount(total_cents),
            currency,
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    })
}

fn observed_string(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<String>> {
    let (line, value) = find_label_value(lines, labels)?;
    Some(observed_from_line(line, value, confidence, document))
}

fn observed_receipt_date(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<String>> {
    let (line, value) = find_label_or_pipe_value(lines, labels)?;
    let value = normalize_receipt_date_value(&value);
    Some(observed_from_line(line, value, confidence, document))
}

fn observed_money(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    let (line, value) = find_label_value(lines, labels)?;
    let money = parse_money_with_line_context(&line.raw, &value)?;
    Some(observed_from_line(line, money, confidence, document))
}

fn observed_money_strict(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    for line in lines {
        let Some(value) = strip_label_value_strict(&line.raw, labels) else {
            continue;
        };
        let normalized_value = normalize_money_value_fragment(&value);
        let Some(money) = parse_money_with_line_context(&line.raw, &normalized_value) else {
            continue;
        };
        return Some(observed_from_line(line, money, confidence, document));
    }

    None
}

fn observed_receipt_total(
    document: &TranscribedDocument,
    lines: &[LineRef],
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    let preferred_label_sets = [
        &["grand total", "net total"][..],
        &["final total", "rounded total", "total rounded"][..],
        &["total includes gst", "total including gst"][..],
        &["total paid", "amount paid"][..],
        &["total sales"][..],
        &["total"][..],
        &["total amt"][..],
    ];

    for labels in preferred_label_sets {
        if let Some(value) = observed_money_strict(document, lines, labels, confidence) {
            return Some(value);
        }
    }

    lines.iter()
        .find_map(|line| observed_receipt_total_from_candidate_line(document, line, confidence))
}

fn observed_receipt_total_from_candidate_line(
    document: &TranscribedDocument,
    line: &LineRef,
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    let normalized = normalize_key(&normalize_bullet_content(&line.raw));
    let is_candidate = normalized.starts_with("cash")
        || normalized.starts_with("grand total")
        || normalized.starts_with("final total")
        || normalized.starts_with("rounded total")
        || normalized.starts_with("total amt rounded")
        || normalized.starts_with("total incl");
    if !is_candidate {
        return None;
    }
    let money = parse_last_money_from_line(&line.raw)?;
    Some(observed_from_line(line, money, confidence, document))
}

fn parse_last_money_from_line(line: &str) -> Option<MoneyAmount> {
    let content = normalize_bullet_content(line);
    for token in content.split_whitespace().rev() {
        if sanitize_amount(token).is_none() {
            continue;
        }
        return parse_money_with_line_context(line, token);
    }
    None
}

fn observed_location(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<Location>> {
    let (line, value) = find_label_value(lines, labels)?;
    let location = parse_location(&value)?;
    Some(observed_from_line(line, location, confidence, document))
}

fn observed_date_range(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<DateRange>> {
    let (line, value) = find_label_value(lines, labels)?;
    let value = parse_date_range(&value)?;
    Some(observed_from_line(line, value, confidence, document))
}

fn observed_date_range_from_labels(
    document: &TranscribedDocument,
    lines: &[LineRef],
    start_labels: &[&str],
    end_labels: &[&str],
    confidence: ConfidenceLevel,
    origin: &str,
) -> Option<Observed<DateRange>> {
    let (start_line, start_value) = find_label_value(lines, start_labels)?;
    let (end_line, end_value) = find_label_value(lines, end_labels)?;
    let mut evidence = vec![
        document_span_evidence(document, start_line),
        document_span_evidence(document, end_line),
    ];
    evidence.push(system_generated_evidence(origin));

    Some(Observed {
        value: DateRange {
            start_date: start_value,
            end_date: end_value,
        },
        confidence,
        evidence,
        flags: Vec::new(),
    })
}

fn observed_pipe_field(
    document: &TranscribedDocument,
    line: &LineRef,
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<String>> {
    let value = find_pipe_value(line, labels)?;
    Some(observed_from_line(line, value, confidence, document))
}

fn observed_pipe_field_money(
    document: &TranscribedDocument,
    line: &LineRef,
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    let value = parse_money(&find_pipe_value(line, labels)?)?;
    Some(observed_from_line(line, value, confidence, document))
}

fn observed_from_line<T>(
    line: &LineRef,
    value: T,
    confidence: ConfidenceLevel,
    document: &TranscribedDocument,
) -> Observed<T> {
    Observed {
        value,
        confidence,
        evidence: vec![document_span_evidence(document, line)],
        flags: Vec::new(),
    }
}

fn observed_from_region<T>(
    document: &TranscribedDocument,
    page: &TranscribedPage,
    region: &TranscribedRegion,
    value: T,
    confidence: ConfidenceLevel,
    flags: Vec<String>,
) -> Observed<T> {
    Observed {
        value,
        confidence,
        evidence: vec![document_region_evidence(document, page, region)],
        flags,
    }
}

fn find_line_with_any<'a>(lines: &'a [LineRef], needles: &[&str]) -> Option<&'a LineRef> {
    lines.iter().find(|line| {
        let haystack = normalize_key(&line.normalized);
        needles
            .iter()
            .any(|needle| haystack.contains(&normalize_key(needle)))
    })
}

fn collect_section_rows<'a>(lines: &'a [LineRef], headings: &[&str]) -> Vec<&'a LineRef> {
    let mut in_section = false;
    let mut collected = Vec::new();

    for line in lines {
        if line.is_heading {
            let is_target = headings
                .iter()
                .any(|heading| matches_heading(line, heading));
            if is_target {
                in_section = true;
                continue;
            }

            if in_section {
                break;
            }
        }

        if in_section {
            collected.push(line);
        }
    }

    collected
}

fn merge_wrapped_pipe_rows(lines: &[&LineRef]) -> Vec<LineRef> {
    let mut merged = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        let current = lines[index];
        let mut combined = normalize_bullet_content(&current.raw);
        let mut last_raw = current.raw.clone();
        let mut next_index = index + 1;

        while combined.ends_with('|') && next_index < lines.len() {
            let continuation = lines[next_index];
            let continuation_content = normalize_bullet_content(&continuation.raw);
            if !is_pipe_row_continuation(&continuation_content) {
                break;
            }

            combined.push(' ');
            combined.push_str(&continuation_content);
            last_raw.push(' ');
            last_raw.push_str(continuation.raw.trim());
            next_index += 1;
        }

        merged.push(LineRef {
            page_number: current.page_number,
            raw: last_raw,
            normalized: normalize_line(&combined),
            is_heading: false,
            is_bullet: current.is_bullet,
        });
        index = next_index;
    }

    merged
}

fn is_pipe_row_continuation(value: &str) -> bool {
    let normalized = normalize_key(value);
    normalized.starts_with("taxes & fees")
        || normalized.starts_with("taxes and fees")
        || normalized.starts_with("taxes")
        || normalized.starts_with("fees")
}

fn find_label_value<'a>(lines: &'a [LineRef], labels: &[&str]) -> Option<(&'a LineRef, String)> {
    for line in lines {
        if let Some(value) = strip_label_value(&line.raw, labels, !line.is_heading) {
            return Some((line, value));
        }
    }
    None
}

fn find_label_or_pipe_value<'a>(
    lines: &'a [LineRef],
    labels: &[&str],
) -> Option<(&'a LineRef, String)> {
    for line in lines {
        if let Some(value) = strip_label_value(&line.raw, labels, !line.is_heading) {
            return Some((line, value));
        }
        if let Some(value) = find_pipe_value(line, labels) {
            return Some((line, value));
        }
    }
    None
}

fn strip_label_value(line: &str, labels: &[&str], allow_loose_prefix: bool) -> Option<String> {
    let content = normalize_bullet_content(line);
    let lhs_rhs = content.split_once(':');
    for label in labels {
        let label_key = normalize_key(label);
        if let Some((lhs, rhs)) = lhs_rhs {
            if normalize_key(lhs) == label_key {
                return Some(rhs.trim().to_owned());
            }
        }

        if !allow_loose_prefix {
            continue;
        }

        let raw_tokens = content.split_whitespace().collect::<Vec<_>>();
        let mut matched_prefix_len = None;
        let mut normalized_prefix = String::new();

        for (index, raw_token) in raw_tokens.iter().enumerate() {
            let token_key = normalize_key(raw_token);
            if token_key.is_empty() {
                continue;
            }

            if normalized_prefix.is_empty() {
                normalized_prefix = token_key;
            } else {
                normalized_prefix.push(' ');
                normalized_prefix.push_str(&token_key);
            }

            if normalized_prefix == label_key {
                matched_prefix_len = Some(index + 1);
                break;
            }

            if !label_key.starts_with(&normalized_prefix) {
                break;
            }
        }

        if let Some(prefix_len) = matched_prefix_len {
            if raw_tokens.len() <= prefix_len {
                continue;
            }

            let next_token_key = normalize_label_continuation_token(raw_tokens[prefix_len]);
            if is_disallowed_label_continuation(&next_token_key) {
                continue;
            }

            let value = raw_tokens[prefix_len..].join(" ");
            let value = value
                .trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch == '#')
                .trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

fn strip_label_value_strict(line: &str, labels: &[&str]) -> Option<String> {
    let content = normalize_bullet_content(line);
    let lhs_rhs = content.split_once(':');
    for label in labels {
        let label_key = normalize_key(label);
        if let Some((lhs, rhs)) = lhs_rhs {
            if normalize_key(lhs) == label_key {
                return Some(rhs.trim().to_owned());
            }
        }

        let raw_tokens = content.split_whitespace().collect::<Vec<_>>();
        let mut matched_prefix_len = None;
        let mut normalized_prefix = String::new();

        for (index, raw_token) in raw_tokens.iter().enumerate() {
            let token_key = normalize_key(raw_token);
            if token_key.is_empty() {
                continue;
            }

            if normalized_prefix.is_empty() {
                normalized_prefix = token_key;
            } else {
                normalized_prefix.push(' ');
                normalized_prefix.push_str(&token_key);
            }

            if normalized_prefix == label_key {
                matched_prefix_len = Some(index + 1);
                break;
            }

            if !label_key.starts_with(&normalized_prefix) {
                break;
            }
        }

        if let Some(prefix_len) = matched_prefix_len {
            if raw_tokens.len() <= prefix_len {
                continue;
            }

            let next_token_key = normalize_label_continuation_token(raw_tokens[prefix_len]);
            if is_disallowed_label_continuation(&next_token_key) {
                continue;
            }

            let value = raw_tokens[prefix_len..].join(" ");
            let value = value
                .trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch == '#')
                .trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }

    None
}

fn find_pipe_value(line: &LineRef, labels: &[&str]) -> Option<String> {
    split_columns(&normalize_bullet_content(&line.raw))
        .into_iter()
        .find_map(|part| strip_label_value(part.trim(), labels, true))
}

fn normalize_label_continuation_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn is_disallowed_label_continuation(value: &str) -> bool {
    matches!(
        value,
        "amt"
            | "amount"
            | "paid"
            | "due"
            | "item"
            | "items"
            | "incl"
            | "including"
            | "excl"
            | "excluding"
            | "rounded"
            | "rounding"
            | "adj"
            | "qty"
            | "quantity"
            | "gst"
            | "tax"
            | "sales"
    )
}

fn normalize_line(value: &str) -> String {
    let trimmed = normalize_bullet_content(value);
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_bullet_content(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim()
        .to_owned()
}

fn looks_like_heading(value: &str) -> bool {
    if value.trim_start().starts_with('#') {
        return true;
    }

    if value.trim_start().starts_with('-') || value.trim_start().starts_with('*') {
        return false;
    }

    let normalized = normalize_key(&normalize_bullet_content(value));
    let heading_candidates = [
        "passenger",
        "passenger details",
        "trip summary",
        "segments",
        "flight segments",
        "stay summary",
        "nightly charges",
        "charges",
        "line items",
        "items",
        "items purchased",
        "purchase summary",
        "meals included",
        "included in rate",
        "included in room rate",
        "guest folio",
        "hotel folio",
        "merchant receipt",
        "card receipt",
        "e ticket itinerary receipt",
        "e ticket itinerary",
    ];

    if heading_candidates
        .iter()
        .any(|candidate| normalized == normalize_key(candidate))
    {
        return true;
    }

    let content = normalize_bullet_content(value);
    !content.contains(':')
        && !content.contains('|')
        && !content.chars().any(|ch| ch.is_ascii_digit())
        && content
            .chars()
            .filter(|ch| ch.is_ascii_alphabetic())
            .count()
            >= 3
        && content
            .chars()
            .filter(|ch| ch.is_ascii_alphabetic())
            .all(|ch| ch.is_ascii_uppercase())
}

fn matches_heading(line: &LineRef, heading: &str) -> bool {
    normalize_key(&line.normalized) == normalize_key(heading)
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'A'..='Z' => ch.to_ascii_lowercase(),
            '&' => '&',
            ':' | '.' | '/' | '-' => ' ',
            _ => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_money(value: &str) -> Option<MoneyAmount> {
    let cleaned = value.split_whitespace().collect::<Vec<_>>();
    if cleaned.is_empty() {
        return None;
    }

    if cleaned.len() >= 2 && cleaned[0].chars().all(|ch| ch.is_ascii_alphabetic()) {
        Some(MoneyAmount {
            currency: Some(normalize_currency_code(cleaned[0])),
            amount: sanitize_amount(&cleaned[1..].join(" "))?,
        })
    } else {
        Some(MoneyAmount {
            currency: None,
            amount: sanitize_amount(value)?,
        })
    }
}

fn parse_money_with_line_context(line: &str, value: &str) -> Option<MoneyAmount> {
    let mut money = parse_money(value)?;
    if money.currency.is_none() {
        money.currency = extract_currency_hint(line);
    }
    Some(money)
}

fn normalize_money_value_fragment(value: &str) -> String {
    let trimmed = value.trim();
    if let Some((prefix, suffix)) = trimmed.rsplit_once(':') {
        let prefix = prefix.trim();
        let suffix = suffix.trim();
        let prefix_is_annotation = prefix.ends_with('%')
            || prefix
                .chars()
                .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '%' | ' '));
        if prefix_is_annotation && sanitize_amount(suffix).is_some() {
            return suffix.to_owned();
        }
    }
    trimmed.to_owned()
}

fn sanitize_amount(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',')
        .collect::<String>()
        .replace(',', "");
    if normalized.is_empty() {
        None
    } else {
        let normalized = if normalized.contains('.') {
            let parts = normalized.split('.').collect::<Vec<_>>();
            if parts.len() > 2 {
                let fractional = parts.last()?.trim();
                if fractional.is_empty() {
                    return None;
                }
                let whole = parts[..parts.len() - 1].join("");
                format!("{whole}.{fractional}")
            } else {
                normalized
            }
        } else {
            format!("{normalized}.00")
        };
        let normalized = normalized.trim_start_matches('.').to_owned();
        if normalized.is_empty() {
            None
        } else if normalized.starts_with('.') {
            Some(format!("0{normalized}"))
        } else {
            Some(normalized)
        }
    }
}

fn normalize_currency_code(value: &str) -> String {
    if value.contains('원') || value.contains('₩') {
        return "KRW".to_owned();
    }

    match value.to_ascii_uppercase().as_str() {
        "RM" => "MYR".to_owned(),
        "WON" => "KRW".to_owned(),
        other => other.to_owned(),
    }
}

fn extract_currency_hint(value: &str) -> Option<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim())
        .find(|token| {
            matches!(
                token.to_ascii_uppercase().as_str(),
                "RM" | "MYR" | "USD" | "SGD" | "JPY" | "GBP" | "EUR" | "CAD" | "AUD"
                    | "KRW" | "WON"
            )
        })
        .map(normalize_currency_code)
}

fn receipt_currency_hint(lines: &[LineRef]) -> Option<String> {
    lines
        .iter()
        .find_map(|line| extract_currency_hint(&line.raw))
}

fn with_money_currency_hint(
    value: Option<Observed<MoneyAmount>>,
    currency_hint: Option<&str>,
    origin: &str,
) -> Option<Observed<MoneyAmount>> {
    let mut value = value?;
    if value.value.currency.is_some() {
        return Some(value);
    }
    let Some(currency_hint) = currency_hint else {
        return Some(value);
    };
    value.value.currency = Some(normalize_currency_code(currency_hint));
    value.evidence.push(system_generated_evidence(origin));
    Some(value)
}

fn parse_date_range(value: &str) -> Option<DateRange> {
    let (start_date, end_date) = value.split_once(" to ")?;
    Some(DateRange {
        start_date: start_date.trim().to_owned(),
        end_date: end_date.trim().to_owned(),
    })
}

fn parse_location(value: &str) -> Option<Location> {
    let mut base = value.trim().to_owned();
    let mut airport_code = None;

    if let Some(start) = base.rfind('(') {
        if base.ends_with(')') {
            airport_code = Some(base[start + 1..base.len() - 1].trim().to_owned());
            base = base[..start].trim().to_owned();
        }
    }

    let parts = base
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() && airport_code.is_none() {
        return None;
    }

    let (city, region, country) = match parts.len() {
        0 => (None, None, None),
        1 => (Some(parts[0].to_owned()), None, None),
        2 => (Some(parts[0].to_owned()), None, Some(parts[1].to_owned())),
        _ => (
            Some(parts[0].to_owned()),
            Some(parts[1].to_owned()),
            Some(parts[parts.len() - 1].to_owned()),
        ),
    };

    Some(Location {
        city,
        region,
        country,
        airport_code,
    })
}

fn split_columns(value: &str) -> Vec<String> {
    if value.contains('|') {
        return value
            .split('|')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }

    let mut columns = Vec::new();
    let mut current = String::new();
    let mut space_run = 0usize;

    for ch in value.chars() {
        if ch == ' ' {
            space_run += 1;
            if space_run == 1 {
                current.push(ch);
            } else if !current.trim().is_empty() {
                columns.push(current.trim().to_owned());
                current.clear();
            }
            continue;
        }

        if space_run > 1 && !current.is_empty() {
            current.push(' ');
        }
        space_run = 0;
        current.push(ch);
    }

    if !current.trim().is_empty() {
        columns.push(current.trim().to_owned());
    }

    if columns.len() <= 1 {
        vec![value.trim().to_owned()]
    } else {
        columns
    }
}

fn split_description_and_money(value: &str) -> Option<(String, MoneyAmount)> {
    let tokens = value.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 {
        return None;
    }

    let amount = sanitize_amount(tokens.last()?)?;
    let mut description_end = tokens.len() - 1;
    let mut currency = None;

    if tokens.len() >= 2 {
        let maybe_currency = tokens[tokens.len() - 2];
        if maybe_currency.len() == 3 && maybe_currency.chars().all(|ch| ch.is_ascii_alphabetic()) {
            currency = Some(maybe_currency.to_ascii_uppercase());
            description_end -= 1;
        }
    }

    let description = tokens[..description_end].join(" ");
    let description = description
        .trim_end_matches(|ch: char| ch == '.' || ch == ':' || ch == '-')
        .trim();
    if description.is_empty() {
        return None;
    }

    Some((description.to_owned(), MoneyAmount { amount, currency }))
}

fn normalize_receipt_date_value(value: &str) -> String {
    let trimmed = value.trim();
    for token in trimmed.split_whitespace() {
        let token = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | '(' | ')'));
        if looks_like_date_token(token) {
            return token.to_owned();
        }
    }

    trimmed.to_owned()
}

fn looks_like_date_token(value: &str) -> bool {
    let separators = ['/', '-'];
    separators.iter().any(|separator| {
        let parts = value.split(*separator).collect::<Vec<_>>();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
    })
}

fn infer_heading_value(
    document: &TranscribedDocument,
    lines: &[LineRef],
    excluded_headings: &[&str],
) -> Option<Observed<String>> {
    lines
        .iter()
        .find(|line| {
            line.is_heading
                && !excluded_headings
                    .iter()
                    .any(|excluded| normalize_key(&line.normalized) == normalize_key(excluded))
        })
        .map(|line| {
            observed_from_line(
                line,
                normalize_bullet_content(&line.raw),
                ConfidenceLevel::Medium,
                document,
            )
        })
}

fn infer_receipt_merchant_name(
    document: &TranscribedDocument,
    lines: &[LineRef],
) -> Option<Observed<String>> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.is_heading)
        .filter(|(_, line)| !is_generic_receipt_heading(&line.normalized))
        .max_by_key(|(index, line)| receipt_heading_score(&line.raw) * 1000 - *index as i32)
        .map(|(_, line)| {
            observed_from_line(
                line,
                normalize_bullet_content(&line.raw),
                ConfidenceLevel::Medium,
                document,
            )
        })
}

fn is_generic_receipt_heading(value: &str) -> bool {
    let normalized = normalize_key(value);
    [
        "merchant receipt",
        "card receipt",
        "receipt",
        "purchase summary",
        "line items",
        "items",
        "totals",
        "footer",
        "merchant details",
        "receipt information",
        "thank you",
        "thank you for shopping",
        "goods sold are not returnable",
        "goods sold are not returnable thank you",
    ]
    .iter()
    .any(|heading| normalized == normalize_key(heading))
}

fn receipt_heading_score(value: &str) -> i32 {
    let content = normalize_bullet_content(value);
    let normalized = normalize_key(&content);
    let business_keywords = [
        "sdn",
        "bhd",
        "inc",
        "llc",
        "shop",
        "store",
        "gift",
        "home",
        "deco",
        "restaurant",
        "bistro",
        "cafe",
        "coffee",
        "diy",
        "mart",
        "market",
        "trading",
        "enterprise",
        "perniagaan",
        "motor",
        "machinery",
    ];

    let mut score = 0i32;
    if business_keywords
        .iter()
        .any(|keyword| normalized.contains(&normalize_key(keyword)))
    {
        score += 10;
    }
    if content.contains('&') || content.contains('(') {
        score += 2;
    }

    let alphabetic = content
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .count() as i32;
    let uppercase = content.chars().filter(|ch| ch.is_ascii_uppercase()).count() as i32;
    if alphabetic > 0 && uppercase * 10 >= alphabetic * 6 {
        score += 5;
    }

    if content.split_whitespace().count() <= 3 && score == 0 {
        score -= 3;
    }

    score
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

fn classification_from_line(
    document: &TranscribedDocument,
    kind: DocumentKind,
    line: &LineRef,
    confidence: ConfidenceLevel,
) -> DocumentClassification {
    DocumentClassification {
        kind,
        confidence,
        evidence: vec![document_span_evidence(document, line)],
        flags: Vec::new(),
    }
}

fn classification_from_region(
    document: &TranscribedDocument,
    kind: DocumentKind,
    page: &TranscribedPage,
    region: &TranscribedRegion,
    confidence: ConfidenceLevel,
    flags: Vec<String>,
) -> DocumentClassification {
    DocumentClassification {
        kind,
        confidence,
        evidence: vec![document_region_evidence(document, page, region)],
        flags,
    }
}

fn document_span_evidence(document: &TranscribedDocument, line: &LineRef) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::DocumentSpan,
        document_id: Some(document.document_id.clone()),
        filename: Some(document.filename.clone()),
        page: Some(line.page_number),
        quote: Some(line.raw.clone()),
        origin: None,
    }
}

fn document_region_evidence(
    document: &TranscribedDocument,
    page: &TranscribedPage,
    region: &TranscribedRegion,
) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::DocumentSpan,
        document_id: Some(document.document_id.clone()),
        filename: Some(document.filename.clone()),
        page: Some(page.page_number),
        quote: Some(region.text.clone()),
        origin: None,
    }
}

fn system_generated_evidence(origin: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::SystemGenerated,
        document_id: None,
        filename: None,
        page: None,
        quote: None,
        origin: Some(origin.to_owned()),
    }
}

fn missing_field_issue(code: &str, message: &str) -> DocumentExtractionIssue {
    DocumentExtractionIssue {
        severity: IssueSeverity::Error,
        code: code.to_owned(),
        message: message.to_owned(),
        evidence: vec![system_generated_evidence("document_extract.missing_field")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_facts::{render_document_facts_json_pretty, DocumentFactsPayload};
    use crate::synthetic_documents::{
        generate_synthetic_document, generate_synthetic_packet, SyntheticVariant,
    };
    use crate::transcribe::{
        OcrBoundingBox, OcrGeometrySource, OcrPassKind, OcrPreprocessVariant, OcrRegionKind,
        PageDimensions, TranscribedPage, TranscribedRegion, TranscriptionEngine,
        TranscriptionMetadata,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_fixture(markdown: &str, filename: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let serial = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("expense_report_schema_{unique}_{serial}"));
        fs::create_dir_all(&dir).expect("temp dir should be creatable");
        let path = dir.join(filename);
        fs::write(&path, markdown).expect("fixture should be writable");
        path
    }

    fn grounded_receipt_document(
        text: &str,
        merchant: &str,
        date: &str,
        total: &str,
        total_currency: Option<&str>,
    ) -> TranscribedDocument {
        let mut regions = vec![
            TranscribedRegion {
                region_id: "merchant_name".to_owned(),
                kind: OcrRegionKind::ValueCandidate,
                text: merchant.to_owned(),
                bbox: Some(OcrBoundingBox {
                    left: 0.1,
                    top: 0.1,
                    width: 0.4,
                    height: 0.05,
                }),
            },
            TranscribedRegion {
                region_id: "transaction_date".to_owned(),
                kind: OcrRegionKind::ValueCandidate,
                text: date.to_owned(),
                bbox: Some(OcrBoundingBox {
                    left: 0.1,
                    top: 0.2,
                    width: 0.3,
                    height: 0.05,
                }),
            },
            TranscribedRegion {
                region_id: "total_paid".to_owned(),
                kind: OcrRegionKind::ValueCandidate,
                text: total.to_owned(),
                bbox: Some(OcrBoundingBox {
                    left: 0.6,
                    top: 0.8,
                    width: 0.2,
                    height: 0.05,
                }),
            },
        ];
        if let Some(total_currency) = total_currency {
            regions.push(TranscribedRegion {
                region_id: "total_paid_currency".to_owned(),
                kind: OcrRegionKind::ValueCandidate,
                text: total_currency.to_owned(),
                bbox: Some(OcrBoundingBox {
                    left: 0.52,
                    top: 0.8,
                    width: 0.05,
                    height: 0.05,
                }),
            });
        }

        TranscribedDocument {
            document_id: "grounded_receipt".to_owned(),
            filename: "grounded_receipt.png".to_owned(),
            source_path: PathBuf::from("grounded_receipt.png"),
            engine: TranscriptionEngine::VertexGeminiSdk,
            metadata: TranscriptionMetadata {
                pass_id: "grounded_receipt_primary_original".to_owned(),
                pass_kind: OcrPassKind::Primary,
                preprocess_variant: OcrPreprocessVariant::Original,
                grounding_preprocess_variant: None,
                producer: "test".to_owned(),
                model: Some("gemini-3-flash-preview".to_owned()),
                geometry_source: OcrGeometrySource::Gemini,
                geometry_available: true,
            },
            pages: vec![TranscribedPage {
                page_number: 1,
                text: text.to_owned(),
                dimensions: Some(PageDimensions {
                    width: 1200,
                    height: 1800,
                }),
                regions,
            }],
        }
    }

    #[test]
    fn round_trips_baseline_synthetic_packet() {
        for fixture in generate_synthetic_packet(SyntheticVariant::Baseline) {
            let path = write_fixture(&fixture.markdown, &fixture.filename);
            let actual = extract_document_facts_path(&path).expect("fixture should transcribe");
            assert_eq!(actual, fixture.expected_facts);
            assert!(actual.validate_contract().is_ok());
            remove_fixture_dir(&path);
        }
    }

    #[test]
    fn round_trips_noisy_synthetic_packet() {
        for fixture in generate_synthetic_packet(SyntheticVariant::Noisy) {
            let path = write_fixture(&fixture.markdown, &fixture.filename);
            let actual = extract_document_facts_path(&path).expect("fixture should transcribe");
            assert_eq!(actual, fixture.expected_facts);
            assert!(actual.validate_contract().is_ok());
            remove_fixture_dir(&path);
        }
    }

    #[test]
    fn flight_extractor_infers_trip_window_from_segments() {
        let mut fixture =
            generate_synthetic_document(DocumentKind::FlightItinerary, SyntheticVariant::Baseline);
        fixture.markdown = fixture
            .markdown
            .replace("- Trip Window: 2025-04-21 to 2025-04-29\n", "");
        let path = write_fixture(&fixture.markdown, &fixture.filename);
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::FlightItinerary(facts) => {
                let trip_window = facts.trip_window.expect("trip window should be inferred");
                assert_eq!(trip_window.value.start_date, "2025-04-21");
                assert_eq!(trip_window.value.end_date, "2025-04-29");
                assert_eq!(trip_window.confidence, ConfidenceLevel::Medium);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn hotel_extractor_infers_total_from_nightly_charges() {
        let mut fixture =
            generate_synthetic_document(DocumentKind::HotelFolio, SyntheticVariant::Baseline);
        fixture.markdown = fixture.markdown.replace("- Total Paid: SGD 778.80\n", "");
        let path = write_fixture(&fixture.markdown, &fixture.filename);
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::HotelFolio(facts) => {
                let total_paid = facts.total_paid.expect("total should be inferred");
                assert_eq!(total_paid.value.amount, "778.80");
                assert_eq!(total_paid.value.currency.as_deref(), Some("SGD"));
                assert_eq!(total_paid.confidence, ConfidenceLevel::Medium);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn hotel_extractor_merges_wrapped_pipe_row_continuations() {
        let mut fixture =
            generate_synthetic_document(DocumentKind::HotelFolio, SyntheticVariant::Baseline);
        fixture.markdown = fixture.markdown.replace(
            "- Date: 2025-04-21 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60",
            "- Date: 2025-04-21 | Description: Deluxe King Room | Room Rate: SGD 220.00 |\n- Taxes & Fees: SGD 39.60",
        );
        fixture.markdown = fixture.markdown.replace(
            "- Date: 2025-04-22 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60",
            "- Date: 2025-04-22 | Description: Deluxe King Room | Room Rate: SGD 220.00 |\n- Taxes & Fees: SGD 39.60",
        );
        fixture.markdown = fixture.markdown.replace(
            "- Date: 2025-04-23 | Description: Deluxe King Room | Room Rate: SGD 220.00 | Taxes & Fees: SGD 39.60",
            "- Date: 2025-04-23 | Description: Deluxe King Room | Room Rate: SGD 220.00 |\n- Taxes & Fees: SGD 39.60",
        );
        let path = write_fixture(&fixture.markdown, &fixture.filename);
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::HotelFolio(facts) => {
                assert_eq!(facts.nightly_charges.len(), 3);
                for night in facts.nightly_charges {
                    assert_eq!(
                        night
                            .room_rate
                            .as_ref()
                            .map(|value| value.value.amount.as_str()),
                        Some("220.00")
                    );
                    assert_eq!(
                        night
                            .taxes_and_fees
                            .first()
                            .map(|value| value.value.amount.as_str()),
                        Some("39.60")
                    );
                }
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_infers_total_from_subtotal_tax_and_tip() {
        let mut fixture =
            generate_synthetic_document(DocumentKind::Receipt, SyntheticVariant::Baseline);
        fixture.markdown = fixture.markdown.replace("- Total Paid: SGD 35.02\n", "");
        let path = write_fixture(&fixture.markdown, &fixture.filename);
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                let total_paid = facts.total_paid.expect("total should be inferred");
                assert_eq!(total_paid.value.amount, "35.02");
                assert_eq!(total_paid.value.currency.as_deref(), Some("SGD"));
                assert_eq!(total_paid.confidence, ConfidenceLevel::Medium);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_fallback_classification_handles_unlabeled_realistic_receipt_layout() {
        let markdown = "\
# EAST BAY BISTRO

- Date: 2025-04-24
- Card: VISA 4242
- Laksa Lunch | SGD 18.00
- Iced Tea | SGD 6.00
- Subtotal: SGD 24.00
- Tax: SGD 2.16
- Tip: SGD 3.00
- Total: SGD 29.16
";
        let path = write_fixture(markdown, "realistic_receipt.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        assert_eq!(actual.classification.kind, DocumentKind::Receipt);
        assert_eq!(actual.classification.confidence, ConfidenceLevel::Medium);

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .merchant_name
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("EAST BAY BISTRO")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("29.16")
                );
                assert_eq!(facts.line_items.len(), 2);
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_prefers_final_or_rounded_total_over_intermediate_total() {
        let markdown = "\
# INDAH GIFT & HOME DECO

- Date: 19/10/2018
- TOTAL AMT: RM 60.31
- ROUNDING ADJ: -0.01
- Final Total: RM 60.30
- CASH: RM 70.30
";
        let path = write_fixture(markdown, "receipt_final_total.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .and_then(|value| value.value.currency.as_deref()),
                    Some("MYR")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("60.30")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_normalizes_datetime_value_to_date_token() {
        let markdown = "\
# BOOK TALK (TAMAN DAYA) SDN BHD

- Date: 25/12/2018 8:13:39 PM
- Total: 9.00
";
        let path = write_fixture(markdown, "receipt_datetime.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("25/12/2018")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_keeps_currency_from_label_context() {
        let markdown = "\
# BOOK TALK (TAMAN DAYA) SDN BHD

- Date: 25/12/2018 8:13:39 PM
- Rounded Total (RM): 9.00
";
        let path = write_fixture(markdown, "receipt_label_currency.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .and_then(|value| value.value.currency.as_deref()),
                    Some("MYR")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("9.00")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn grounded_receipt_regions_classify_and_extract_multilingual_receipt() {
        let document = grounded_receipt_document(
            "# 스타필드\n영수증\n합계 60,000원",
            "Starfield",
            "2025-10-03",
            "60,000",
            Some("원"),
        );
        let actual = extract_document_facts(&document);

        assert_eq!(actual.classification.kind, DocumentKind::Receipt);
        assert_eq!(actual.classification.confidence, ConfidenceLevel::Medium);

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .merchant_name
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("Starfield")
                );
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("2025-10-03")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("60000.00")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .and_then(|value| value.value.currency.as_deref()),
                    Some("KRW")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn grounded_receipt_regions_infer_krw_without_explicit_currency_region() {
        let document = grounded_receipt_document(
            "# 스타필드\n영수증\n합계 60,000원",
            "Starfield",
            "2025-10-03",
            "60,000",
            None,
        );
        let actual = extract_document_facts(&document);

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                let total_paid = facts.total_paid.expect("total should be extracted");
                assert_eq!(total_paid.value.amount, "60000.00");
                assert_eq!(total_paid.value.currency.as_deref(), Some("KRW"));
                assert!(total_paid
                    .flags
                    .iter()
                    .any(|flag| flag == "hangul_currency_inference"));
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn receipt_extractor_prefers_business_heading_over_customer_name_heading() {
        let markdown = "\
# tan woon yann

## INDAH GIFT & HOME DECO
- Date: 19/10/2018
- TOTAL AMT: RM 60.31
- TOTAL: RM 60.30
";
        let path = write_fixture(markdown, "receipt_heading_priority.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .merchant_name
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("INDAH GIFT & HOME DECO")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("60.30")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_ignores_boilerplate_heading_candidates() {
        let markdown = "\
# SAM SAM TRADING CO
(742016-W)

- TOTAL: RM 14.10
- Date: Friday, 29-12-2017

THANK YOU FOR SHOPPING
GOODS SOLD ARE NOT RETURNABLE.
";
        let path = write_fixture(markdown, "receipt_boilerplate_heading.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .merchant_name
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("SAM SAM TRADING CO")
                );
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("29-12-2017")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_extracts_date_from_pipe_summary_lines() {
        let markdown = "\
# SOON HUAT MACHINERY ENTERPRISE

- Doc No.: CS00004040 | Date: 11/01/2019
- Total Sales: 327.00
";
        let path = write_fixture(markdown, "receipt_pipe_date.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("11/01/2019")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("327.00")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_prefers_general_total_over_total_gst_and_total_qty() {
        let markdown = "\
# PERNIAGAAN ZHENG HUI

- Total Qty: 9 | 327.00
- Total GST (RM): 6.37
- Total (RM): 112.45
- Date: 12/02/2018
";
        let path = write_fixture(markdown, "receipt_total_disambiguation.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("112.45")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_accepts_total_includes_gst_label() {
        let markdown = "\
# TAX INVOICE

## Merchant Details
- Merchant Name: FUYI MINI MARKET

## Transaction Summary
- Date: 25/01/2018 1:22:56PM

## Totals
- Total Includes GST 6%: 9.00
";
        let path = write_fixture(markdown, "receipt_total_includes_gst.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("9.00")
                );
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("25/01/2018")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_ignores_total_excl_and_preserves_rm_total() {
        let markdown = "\
# Merchant Receipt

- Merchant: HOME MASTER HARDWARE & ELECTRICAL

- Date: 22/12/2017 14:03
- Total Excl. Of GST 15.00
- Total Incl. Of GST RM 15.90
- Total Amt Rounded 15.90
";
        let path = write_fixture(markdown, "receipt_total_excl_guard.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("15.90")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .and_then(|value| value.value.currency.as_deref()),
                    Some("MYR")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_ignores_total_items_when_selecting_payable_total() {
        let markdown = "\
# Merchant Receipt

- Merchant: RESTORAN HASSANBISTRO

- Date: 12/28/2017 10:17:32 PM
- Total Items = 1.00
- Total Qty = 1.00
- Total Incl. 6% GST RM 15.00
- CASH RM 15.00
";
        let path = write_fixture(markdown, "receipt_total_items_guard.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("15.00")
                );
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .and_then(|value| value.value.currency.as_deref()),
                    Some("MYR")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_extractor_ignores_total_item_parens_when_selecting_payable_total() {
        let markdown = "\
# Merchant Receipt

- Merchant: HOME MASTER HARDWARE & ELECTRICAL
- Date: 22/12/2017 14:03
- 24MMX7Y M.ONE TAPE | 1.00 x 15.90 | 15.90 | SR
- Subtotal: 15.90
- Total Excl. of GST: 15.00
- Total Incl. of GST: 15.90
- Total Amt Rounded: 15.90
- Payment: 50.00
- Change Due: 34.10
- Total Item(s): 1
";
        let path = write_fixture(markdown, "receipt_total_items_parens_guard.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("15.90")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    #[test]
    fn receipt_fallback_classification_accepts_grand_total_layouts() {
        let markdown = "\
# S.H.H. MOTOR ( SUNGAI RENGIT ) SDN. BHD.

- Date: 23-01-2019 13:14:15 PM
- Grand Total: 20.00
- Cash: 20.00
";
        let path = write_fixture(markdown, "receipt_grand_total.md");
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");

        assert_eq!(actual.classification.kind, DocumentKind::Receipt);

        match actual.facts {
            DocumentFactsPayload::Receipt(facts) => {
                assert_eq!(
                    facts
                        .total_paid
                        .as_ref()
                        .map(|value| value.value.amount.as_str()),
                    Some("20.00")
                );
                assert_eq!(
                    facts
                        .transaction_date
                        .as_ref()
                        .map(|value| value.value.as_str()),
                    Some("23-01-2019")
                );
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        remove_fixture_dir(&path);
    }

    fn remove_fixture_dir(path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::remove_dir_all(parent) {
                assert_eq!(
                    err.kind(),
                    std::io::ErrorKind::NotFound,
                    "fixture dir should be removable: {err}"
                );
            }
        }
    }

    #[test]
    fn extractors_flag_missing_non_derivable_required_fields() {
        let mut flight =
            generate_synthetic_document(DocumentKind::FlightItinerary, SyntheticVariant::Baseline);
        flight.markdown = flight
            .markdown
            .replace("- Traveler Name: Olivia Park\n", "");
        let path = write_fixture(&flight.markdown, &flight.filename);
        let actual = extract_document_facts_path(&path).expect("fixture should transcribe");
        assert_eq!(actual.extraction_status, ExtractionStatus::Partial);
        assert!(actual
            .issues
            .iter()
            .any(|issue| issue.code == "missing_traveler_name"));
        fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
    }

    #[test]
    fn extracted_facts_render_to_json() {
        let fixture =
            generate_synthetic_document(DocumentKind::Receipt, SyntheticVariant::Baseline);
        let rendered = render_document_facts_json_pretty(&fixture.expected_facts)
            .expect("facts should render to json");
        assert!(rendered.contains("\"classification\""));
        assert!(rendered.contains("East Bay Bistro"));
    }

    #[test]
    fn parses_loose_labels_with_hyphen_and_slash_variants() {
        assert_eq!(
            strip_label_value("City/Country Singapore, Singapore", &["city/country"], true),
            Some("Singapore, Singapore".to_owned())
        );
        assert_eq!(
            strip_label_value("Check-in 2025-04-21", &["check in"], true),
            Some("2025-04-21".to_owned())
        );
        assert_eq!(
            strip_label_value("Record Locator H7K9Q2", &["record locator"], true),
            Some("H7K9Q2".to_owned())
        );
    }

    #[test]
    fn splits_receipt_lines_with_currency_suffixes() {
        assert_eq!(
            split_description_and_money("Laksa Lunch ........ SGD 18.00"),
            Some((
                "Laksa Lunch".to_owned(),
                MoneyAmount {
                    amount: "18.00".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
            ))
        );
        assert_eq!(
            split_description_and_money("Service Charge SGD 4.00"),
            Some((
                "Service Charge".to_owned(),
                MoneyAmount {
                    amount: "4.00".to_owned(),
                    currency: Some("SGD".to_owned()),
                },
            ))
        );
    }

    #[test]
    fn uppercase_summary_lines_are_not_misclassified_as_headings() {
        assert!(!looks_like_heading("GST SGD 2.52"));
        assert!(looks_like_heading("EAST BAY BISTRO"));
        assert!(looks_like_heading("Guest Folio"));
    }
}
