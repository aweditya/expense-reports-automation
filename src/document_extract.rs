use std::path::Path;

use crate::document_facts::{
    DateRange, DocumentClassification, DocumentExtractionIssue, DocumentFactsPayload, DocumentKind,
    ExtractedDocumentFacts, ExtractionStatus, FlightItineraryFacts, FlightSegmentFacts,
    HotelFolioFacts, HotelNightChargeFacts, IssueSeverity, Location, MoneyAmount, Observed,
    ReceiptFacts, ReceiptLineItemFacts, UnknownDocumentFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};
use crate::transcribe::{transcribe_document_path, TranscribedDocument, TranscriptionError};

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
            let trimmed = raw_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let is_heading = trimmed.starts_with('#');
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
    if let Some(line) = find_line_with_any(lines, &["itinerary / receipt", "e-ticket itinerary"]) {
        return classification_from_line(document, DocumentKind::FlightItinerary, line, ConfidenceLevel::High);
    }

    if let Some(line) = find_line_with_any(lines, &["hotel folio", "guest folio"]) {
        return classification_from_line(document, DocumentKind::HotelFolio, line, ConfidenceLevel::High);
    }

    if let Some(line) = find_line_with_any(lines, &["merchant receipt", "card receipt"]) {
        return classification_from_line(document, DocumentKind::Receipt, line, ConfidenceLevel::High);
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

fn extract_flight_itinerary(
    document: &TranscribedDocument,
    lines: &[LineRef],
    classification: DocumentClassification,
) -> ExtractedDocumentFacts {
    let traveler_name = observed_string(document, lines, &["traveler name"], ConfidenceLevel::High);
    let confirmation_code =
        observed_string(document, lines, &["booking reference", "confirmation code"], ConfidenceLevel::High);
    let ticket_number = observed_string(document, lines, &["ticket number", "ticket no"], ConfidenceLevel::High);
    let booking_date = observed_string(document, lines, &["booking date", "booked"], ConfidenceLevel::High);
    let departure_location =
        observed_location(document, lines, &["origin", "from"], ConfidenceLevel::High);
    let arrival_location =
        observed_location(document, lines, &["destination", "to"], ConfidenceLevel::High);
    let explicit_trip_window =
        observed_date_range(document, lines, &["trip window", "travel window"], ConfidenceLevel::High);
    let total_paid = observed_money(document, lines, &["total paid", "fare paid"], ConfidenceLevel::High);
    let segment_lines = collect_section_bullets(lines, &["segments"]);
    let segments = segment_lines
        .iter()
        .filter_map(|line| parse_flight_segment(document, line))
        .collect::<Vec<_>>();

    let trip_window = explicit_trip_window.or_else(|| infer_trip_window_from_segments(&segments));
    let mut issues = Vec::new();

    if traveler_name.is_none() {
        issues.push(missing_field_issue("missing_traveler_name", "Flight itinerary is missing traveler name"));
    }
    if segments.is_empty() {
        issues.push(missing_field_issue("missing_segments", "Flight itinerary is missing segment details"));
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
    let guest_name = observed_string(document, lines, &["guest name"], ConfidenceLevel::High);
    let property_name = observed_string(document, lines, &["property name", "hotel name"], ConfidenceLevel::High);
    let folio_number = observed_string(document, lines, &["folio number"], ConfidenceLevel::High);
    let property_location =
        observed_location(document, lines, &["property location", "hotel location"], ConfidenceLevel::High);
    let explicit_stay_window =
        observed_date_range(document, lines, &["stay window", "check-in / check-out"], ConfidenceLevel::High);
    let nightly_lines = collect_section_bullets(lines, &["nightly charges"]);
    let nightly_charges = nightly_lines
        .iter()
        .filter_map(|line| parse_hotel_night_charge(document, line))
        .collect::<Vec<_>>();
    let stay_window = explicit_stay_window.or_else(|| infer_stay_window_from_nights(&nightly_charges));
    let total_paid = observed_money(document, lines, &["total paid", "amount paid"], ConfidenceLevel::High)
        .or_else(|| infer_hotel_total_from_nights(&nightly_charges));
    let meals_included = collect_section_bullets(lines, &["meals included"])
        .into_iter()
        .map(|line| observed_from_line(line, normalize_bullet_content(&line.raw), ConfidenceLevel::High, document))
        .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if guest_name.is_none() {
        issues.push(missing_field_issue("missing_guest_name", "Hotel folio is missing guest name"));
    }
    if property_name.is_none() {
        issues.push(missing_field_issue("missing_property_name", "Hotel folio is missing property name"));
    }
    if nightly_charges.is_empty() {
        issues.push(missing_field_issue("missing_nightly_charges", "Hotel folio is missing nightly charges"));
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
    let merchant_name = observed_string(document, lines, &["merchant name"], ConfidenceLevel::High);
    let merchant_location =
        observed_location(document, lines, &["merchant location", "location"], ConfidenceLevel::High);
    let transaction_date =
        observed_string(document, lines, &["transaction date", "date"], ConfidenceLevel::High);
    let subtotal = observed_money(document, lines, &["subtotal"], ConfidenceLevel::High);
    let tax_amount = observed_money(document, lines, &["tax"], ConfidenceLevel::High);
    let tip_amount = observed_money(document, lines, &["tip"], ConfidenceLevel::High);
    let total_paid = observed_money(document, lines, &["total paid"], ConfidenceLevel::High)
        .or_else(|| infer_receipt_total(subtotal.as_ref(), tax_amount.as_ref(), tip_amount.as_ref()));
    let line_items = collect_section_bullets(lines, &["line items"])
        .into_iter()
        .filter_map(|line| parse_receipt_line_item(document, line))
        .collect::<Vec<_>>();

    let mut issues = Vec::new();
    if merchant_name.is_none() {
        issues.push(missing_field_issue("missing_merchant_name", "Receipt is missing merchant name"));
    }
    if transaction_date.is_none() {
        issues.push(missing_field_issue("missing_transaction_date", "Receipt is missing transaction date"));
    }
    if total_paid.is_none() {
        issues.push(missing_field_issue("missing_total_paid", "Receipt is missing total paid"));
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
    let title_hint = lines
        .iter()
        .find(|line| line.is_heading)
        .map(|line| observed_from_line(line, normalize_bullet_content(&line.raw), ConfidenceLevel::Low, document));
    let text_summary = lines
        .first()
        .map(|line| observed_from_line(line, normalize_bullet_content(&line.raw), ConfidenceLevel::Low, document));

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
            evidence: vec![system_generated_evidence("document_extract.unknown_document")],
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
    if issues.iter().any(|issue| issue.severity == IssueSeverity::Error) {
        ExtractionStatus::Partial
    } else {
        ExtractionStatus::Complete
    }
}

fn parse_flight_segment(document: &TranscribedDocument, line: &LineRef) -> Option<FlightSegmentFacts> {
    let departure_airport = observed_pipe_field(document, line, &["departure airport"], ConfidenceLevel::High)?;
    let arrival_airport = observed_pipe_field(document, line, &["arrival airport"], ConfidenceLevel::High)?;
    let departure_date = observed_pipe_field(document, line, &["departure date"], ConfidenceLevel::High)?;
    let arrival_date = observed_pipe_field(document, line, &["arrival date"], ConfidenceLevel::High);
    let marketing_carrier =
        observed_pipe_field(document, line, &["marketing carrier", "carrier"], ConfidenceLevel::High);
    let flight_number = observed_pipe_field(document, line, &["flight number"], ConfidenceLevel::High);
    let cabin_class = observed_pipe_field(document, line, &["cabin class"], ConfidenceLevel::High);

    Some(FlightSegmentFacts {
        departure_airport,
        arrival_airport,
        departure_date,
        arrival_date,
        marketing_carrier,
        flight_number,
        cabin_class,
    })
}

fn parse_hotel_night_charge(document: &TranscribedDocument, line: &LineRef) -> Option<HotelNightChargeFacts> {
    let date = observed_pipe_field(document, line, &["date"], ConfidenceLevel::High)?;
    let description = observed_pipe_field(document, line, &["description"], ConfidenceLevel::High);
    let room_rate = observed_pipe_field_money(document, line, &["room rate"], ConfidenceLevel::High);
    let taxes_and_fees = observed_pipe_field_money(document, line, &["taxes & fees"], ConfidenceLevel::High)
        .into_iter()
        .collect::<Vec<_>>();

    Some(HotelNightChargeFacts {
        date,
        room_rate,
        taxes_and_fees,
        description,
    })
}

fn parse_receipt_line_item(document: &TranscribedDocument, line: &LineRef) -> Option<ReceiptLineItemFacts> {
    let content = normalize_bullet_content(&line.raw);
    let mut parts = content.split('|').map(str::trim);
    let description = parts.next()?;
    let amount = parts.next()?;
    let amount = parse_money(amount)?;

    Some(ReceiptLineItemFacts {
        description: observed_from_line(line, description.to_owned(), ConfidenceLevel::High, document),
        amount: observed_from_line(line, amount, ConfidenceLevel::High, document),
    })
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
        value: DateRange { start_date, end_date },
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
        value: DateRange { start_date, end_date },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    })
}

fn infer_hotel_total_from_nights(nights: &[HotelNightChargeFacts]) -> Option<Observed<MoneyAmount>> {
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

fn observed_money(
    document: &TranscribedDocument,
    lines: &[LineRef],
    labels: &[&str],
    confidence: ConfidenceLevel,
) -> Option<Observed<MoneyAmount>> {
    let (line, value) = find_label_value(lines, labels)?;
    let money = parse_money(&value)?;
    Some(observed_from_line(line, money, confidence, document))
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

fn find_line_with_any<'a>(lines: &'a [LineRef], needles: &[&str]) -> Option<&'a LineRef> {
    lines.iter().find(|line| {
        let haystack = normalize_key(&line.normalized);
        needles
            .iter()
            .any(|needle| haystack.contains(&normalize_key(needle)))
    })
}

fn collect_section_bullets<'a>(lines: &'a [LineRef], headings: &[&str]) -> Vec<&'a LineRef> {
    let mut in_section = false;
    let mut collected = Vec::new();

    for line in lines {
        if line.is_heading {
            let is_target = headings
                .iter()
                .any(|heading| normalize_key(&line.normalized) == normalize_key(heading));
            if is_target {
                in_section = true;
                continue;
            }

            if in_section {
                break;
            }
        }

        if in_section && line.is_bullet {
            collected.push(line);
        }
    }

    collected
}

fn find_label_value<'a>(lines: &'a [LineRef], labels: &[&str]) -> Option<(&'a LineRef, String)> {
    for line in lines {
        if let Some(value) = strip_label_value(&line.normalized, labels) {
            return Some((line, value));
        }
    }
    None
}

fn strip_label_value(line: &str, labels: &[&str]) -> Option<String> {
    let (lhs, rhs) = line.split_once(':')?;
    let lhs_key = normalize_key(lhs);
    for label in labels {
        if lhs_key == normalize_key(label) {
            return Some(rhs.trim().to_owned());
        }
    }
    None
}

fn find_pipe_value(line: &LineRef, labels: &[&str]) -> Option<String> {
    normalize_bullet_content(&line.raw)
        .split('|')
        .find_map(|part| strip_label_value(part.trim(), labels))
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
            currency: Some(cleaned[0].to_ascii_uppercase()),
            amount: sanitize_amount(&cleaned[1..].join(" "))?,
        })
    } else {
        Some(MoneyAmount {
            currency: None,
            amount: sanitize_amount(value)?,
        })
    }
}

fn sanitize_amount(value: &str) -> Option<String> {
    let normalized = value
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == ',')
        .collect::<String>()
        .replace(',', "");
    if normalized.is_empty() || !normalized.contains('.') {
        None
    } else {
        Some(normalized)
    }
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
    use crate::synthetic_documents::{generate_synthetic_document, generate_synthetic_packet, SyntheticVariant};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_fixture(markdown: &str, filename: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("expense_report_schema_{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be creatable");
        let path = dir.join(filename);
        fs::write(&path, markdown).expect("fixture should be writable");
        path
    }

    #[test]
    fn round_trips_baseline_synthetic_packet() {
        for fixture in generate_synthetic_packet(SyntheticVariant::Baseline) {
            let path = write_fixture(&fixture.markdown, &fixture.filename);
            let actual = extract_document_facts_path(&path).expect("fixture should transcribe");
            assert_eq!(actual, fixture.expected_facts);
            assert!(actual.validate_contract().is_ok());
            fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
        }
    }

    #[test]
    fn round_trips_noisy_synthetic_packet() {
        for fixture in generate_synthetic_packet(SyntheticVariant::Noisy) {
            let path = write_fixture(&fixture.markdown, &fixture.filename);
            let actual = extract_document_facts_path(&path).expect("fixture should transcribe");
            assert_eq!(actual, fixture.expected_facts);
            assert!(actual.validate_contract().is_ok());
            fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
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

        fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
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

        fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
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

        fs::remove_dir_all(path.parent().unwrap()).expect("fixture dir should be removable");
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
        let fixture = generate_synthetic_document(DocumentKind::Receipt, SyntheticVariant::Baseline);
        let rendered = render_document_facts_json_pretty(&fixture.expected_facts)
            .expect("facts should render to json");
        assert!(rendered.contains("\"classification\""));
        assert!(rendered.contains("East Bay Bistro"));
    }
}
