use serde::{Deserialize, Serialize};

use crate::document_facts::{
    DateRange, DocumentClassification, DocumentFactsPayload, DocumentKind, ExtractedDocumentFacts,
    ExtractionStatus, FlightItineraryFacts, FlightSegmentFacts, HotelFolioFacts,
    HotelNightChargeFacts, Location, MoneyAmount, Observed, ReceiptFacts, ReceiptLineItemFacts,
};
use crate::draft::{ConfidenceLevel, EvidenceKind, EvidenceReference};
use crate::synthetic_documents::{SyntheticDocumentFixture, SyntheticVariant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntheticLedgerScenario {
    Accepted,
    ReturnedAndCorrected,
}

impl SyntheticLedgerScenario {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::ReturnedAndCorrected => "returned_and_corrected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripWindowMode {
    Explicit,
    DepartureReturnDerived,
    SegmentDerived,
}

impl TripWindowMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::DepartureReturnDerived => "departure_return_derived",
            Self::SegmentDerived => "segment_derived",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StayWindowMode {
    Explicit,
    CheckInOutDerived,
    NightDerived,
}

impl StayWindowMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::CheckInOutDerived => "check_in_out_derived",
            Self::NightDerived => "night_derived",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TotalMode {
    Explicit,
    Derived,
}

impl TotalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Derived => "derived",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticPacketFixture {
    pub packet_id: String,
    pub variant: String,
    pub scenario: SyntheticLedgerScenario,
    pub destination_city: String,
    pub destination_country: String,
    pub currency: String,
    pub trip_window_mode: TripWindowMode,
    pub stay_window_mode: StayWindowMode,
    pub hotel_total_mode: TotalMode,
    pub receipt_total_mode: TotalMode,
    pub alcohol_receipt: bool,
    pub fixtures: Vec<SyntheticDocumentFixture>,
}

pub fn generate_synthetic_corpus(packet_count: usize) -> Vec<SyntheticPacketFixture> {
    (0..packet_count)
        .map(generate_synthetic_packet_case)
        .collect()
}

fn generate_synthetic_packet_case(index: usize) -> SyntheticPacketFixture {
    let packet_id = format!("synthetic_packet_{:04}", index + 1);
    let traveler = TRAVELERS[index % TRAVELERS.len()];
    let destination = &DESTINATIONS[index % DESTINATIONS.len()];
    let carrier = CARRIERS[index % CARRIERS.len()];
    let schedule = &SCHEDULES[index % SCHEDULES.len()];
    let variant = if index % 2 == 0 {
        SyntheticVariant::Baseline
    } else {
        SyntheticVariant::Noisy
    };
    let scenario = if index % 2 == 0 {
        SyntheticLedgerScenario::Accepted
    } else {
        SyntheticLedgerScenario::ReturnedAndCorrected
    };
    let trip_window_mode = match index % 3 {
        0 => TripWindowMode::Explicit,
        1 => TripWindowMode::DepartureReturnDerived,
        _ => TripWindowMode::SegmentDerived,
    };
    let stay_window_mode = match (index / 2) % 3 {
        0 => StayWindowMode::Explicit,
        1 => StayWindowMode::CheckInOutDerived,
        _ => StayWindowMode::NightDerived,
    };
    let hotel_total_mode = if index % 2 == 0 {
        TotalMode::Explicit
    } else {
        TotalMode::Derived
    };
    let receipt_total_mode = if (index / 3) % 2 == 0 {
        TotalMode::Explicit
    } else {
        TotalMode::Derived
    };
    let alcohol_receipt = index % 4 == 0;

    let room_rate_cents = 18_500 + ((index % 5) as i64 * 2_000);
    let hotel_tax_cents = room_rate_cents * 18 / 100;
    let hotel_total_cents = 3 * (room_rate_cents + hotel_tax_cents);
    let subtotal_cents = 2_400 + ((index % 7) as i64 * 350);
    let tax_cents = subtotal_cents * 9 / 100;
    let tip_cents = if alcohol_receipt { 720 } else { 540 };
    let total_receipt_cents = subtotal_cents + tax_cents + tip_cents;
    let airfare_total_cents = 96_000 + ((index % 6) as i64 * 13_500);
    let service_charge_cents = 300 + ((index % 3) as i64 * 50);
    let second_item_cents = 600;
    let alcohol_item_cents = if alcohol_receipt { 850 } else { 0 };
    let first_item_cents =
        subtotal_cents - service_charge_cents - second_item_cents - alcohol_item_cents;
    let third_item_cents = service_charge_cents;

    let flight_fixture = build_flight_fixture(
        &packet_id,
        traveler,
        destination,
        carrier,
        schedule,
        variant,
        trip_window_mode,
        airfare_total_cents,
    );
    let hotel_fixture = build_hotel_fixture(
        &packet_id,
        traveler,
        destination,
        schedule,
        variant,
        stay_window_mode,
        hotel_total_mode,
        room_rate_cents,
        hotel_tax_cents,
        hotel_total_cents,
    );
    let receipt_fixture = build_receipt_fixture(
        &packet_id,
        destination,
        schedule,
        variant,
        receipt_total_mode,
        subtotal_cents,
        tax_cents,
        tip_cents,
        total_receipt_cents,
        first_item_cents,
        second_item_cents,
        third_item_cents,
        alcohol_item_cents,
        alcohol_receipt,
    );

    SyntheticPacketFixture {
        packet_id,
        variant: variant.as_str().to_owned(),
        scenario,
        destination_city: destination.city.to_owned(),
        destination_country: destination.country.to_owned(),
        currency: destination.currency.to_owned(),
        trip_window_mode,
        stay_window_mode,
        hotel_total_mode,
        receipt_total_mode,
        alcohol_receipt,
        fixtures: vec![flight_fixture, hotel_fixture, receipt_fixture],
    }
}

#[derive(Clone, Copy)]
struct DestinationProfile {
    city: &'static str,
    country: &'static str,
    airport_code: &'static str,
    hotels: &'static [&'static str],
    merchants: &'static [&'static str],
    room_descriptions: &'static [&'static str],
    line_items: &'static [&'static str],
    currency: &'static str,
}

#[derive(Clone, Copy)]
struct CarrierProfile {
    marketing_carrier: &'static str,
    outbound_flight_number: &'static str,
    inbound_flight_number: &'static str,
    cabin_class: &'static str,
}

#[derive(Clone, Copy)]
struct ScheduleProfile {
    booking_date: &'static str,
    departure_date: &'static str,
    outbound_arrival_date: &'static str,
    hotel_nights: [&'static str; 3],
    checkout_date: &'static str,
    receipt_date: &'static str,
    return_date: &'static str,
}

const TRAVELERS: &[&str] = &[
    "Olivia Park",
    "Daniel Kim",
    "Priya Shah",
    "Marcus Nguyen",
    "Elena Rossi",
    "Jordan Lee",
    "Amelia Chen",
    "Noah Patel",
    "Sophia Turner",
    "Isaac Flores",
];

const DESTINATIONS: &[DestinationProfile] = &[
    DestinationProfile {
        city: "Singapore",
        country: "Singapore",
        airport_code: "SIN",
        hotels: &["Marina Bay Grand Hotel", "Riverside Quay Suites"],
        merchants: &["Lion City Bistro", "Harbour View Restaurant"],
        room_descriptions: &["Deluxe King Room", "Harbor View Room"],
        line_items: &["Conference Dinner", "Sparkling Water", "Service Charge"],
        currency: "SGD",
    },
    DestinationProfile {
        city: "Tokyo",
        country: "Japan",
        airport_code: "NRT",
        hotels: &["Shinjuku Central Hotel", "Tokyo Station Garden Inn"],
        merchants: &["Sakura Grill Restaurant", "Ginza Market Cafe"],
        room_descriptions: &["Executive Twin Room", "Garden Superior Room"],
        line_items: &["Set Dinner", "Green Tea", "Service Charge"],
        currency: "JPY",
    },
    DestinationProfile {
        city: "London",
        country: "United Kingdom",
        airport_code: "LHR",
        hotels: &["Kensington Townhouse Hotel", "Westminster Bridge Suites"],
        merchants: &["Thameside Bistro", "West End Restaurant"],
        room_descriptions: &["Classic Queen Room", "Courtyard Double Room"],
        line_items: &["Client Lunch", "Still Water", "Service Charge"],
        currency: "GBP",
    },
    DestinationProfile {
        city: "Paris",
        country: "France",
        airport_code: "CDG",
        hotels: &["Left Bank Residence Hotel", "Opera District Grand"],
        merchants: &["Rive Gauche Cafe", "Seine View Restaurant"],
        room_descriptions: &["Superior Queen Room", "Balcony Deluxe Room"],
        line_items: &["Bistro Lunch", "Mineral Water", "Service Charge"],
        currency: "EUR",
    },
    DestinationProfile {
        city: "Toronto",
        country: "Canada",
        airport_code: "YYZ",
        hotels: &["Harbourfront Executive Hotel", "University Circle Suites"],
        merchants: &["Lakefront Grill", "Maple Street Restaurant"],
        room_descriptions: &["Premier King Room", "City View Studio"],
        line_items: &["Team Dinner", "Iced Tea", "Service Charge"],
        currency: "CAD",
    },
    DestinationProfile {
        city: "Sydney",
        country: "Australia",
        airport_code: "SYD",
        hotels: &["Circular Quay Grand Hotel", "Darling Harbour Residence"],
        merchants: &["Opera House Bistro", "Harbour Lights Restaurant"],
        room_descriptions: &["Skyline King Room", "Harbour Suite"],
        line_items: &["Business Dinner", "Lemon Soda", "Service Charge"],
        currency: "AUD",
    },
];

const CARRIERS: &[CarrierProfile] = &[
    CarrierProfile {
        marketing_carrier: "ANA",
        outbound_flight_number: "NH107",
        inbound_flight_number: "NH108",
        cabin_class: "Economy",
    },
    CarrierProfile {
        marketing_carrier: "United",
        outbound_flight_number: "UA837",
        inbound_flight_number: "UA838",
        cabin_class: "Premium Economy",
    },
    CarrierProfile {
        marketing_carrier: "British Airways",
        outbound_flight_number: "BA286",
        inbound_flight_number: "BA287",
        cabin_class: "Economy",
    },
    CarrierProfile {
        marketing_carrier: "Air France",
        outbound_flight_number: "AF083",
        inbound_flight_number: "AF084",
        cabin_class: "Premium Economy",
    },
    CarrierProfile {
        marketing_carrier: "Air Canada",
        outbound_flight_number: "AC744",
        inbound_flight_number: "AC745",
        cabin_class: "Economy",
    },
    CarrierProfile {
        marketing_carrier: "Qantas",
        outbound_flight_number: "QF074",
        inbound_flight_number: "QF073",
        cabin_class: "Economy",
    },
];

const SCHEDULES: &[ScheduleProfile] = &[
    ScheduleProfile {
        booking_date: "2025-04-10",
        departure_date: "2025-04-21",
        outbound_arrival_date: "2025-04-22",
        hotel_nights: ["2025-04-21", "2025-04-22", "2025-04-23"],
        checkout_date: "2025-04-24",
        receipt_date: "2025-04-24",
        return_date: "2025-04-29",
    },
    ScheduleProfile {
        booking_date: "2025-04-11",
        departure_date: "2025-04-22",
        outbound_arrival_date: "2025-04-23",
        hotel_nights: ["2025-04-22", "2025-04-23", "2025-04-24"],
        checkout_date: "2025-04-25",
        receipt_date: "2025-04-25",
        return_date: "2025-04-29",
    },
    ScheduleProfile {
        booking_date: "2025-04-12",
        departure_date: "2025-04-23",
        outbound_arrival_date: "2025-04-24",
        hotel_nights: ["2025-04-23", "2025-04-24", "2025-04-25"],
        checkout_date: "2025-04-26",
        receipt_date: "2025-04-26",
        return_date: "2025-04-29",
    },
    ScheduleProfile {
        booking_date: "2025-04-13",
        departure_date: "2025-04-24",
        outbound_arrival_date: "2025-04-25",
        hotel_nights: ["2025-04-24", "2025-04-25", "2025-04-26"],
        checkout_date: "2025-04-27",
        receipt_date: "2025-04-27",
        return_date: "2025-04-29",
    },
];

fn build_flight_fixture(
    packet_id: &str,
    traveler: &str,
    destination: &DestinationProfile,
    carrier: CarrierProfile,
    schedule: &ScheduleProfile,
    variant: SyntheticVariant,
    trip_window_mode: TripWindowMode,
    airfare_total_cents: i64,
) -> SyntheticDocumentFixture {
    let filename = format!("{packet_id}_flight_itinerary.md");
    let document_id = document_id_for_filename(&filename);
    let heading = match variant {
        SyntheticVariant::Baseline => "# E-Ticket Itinerary / Receipt",
        SyntheticVariant::Noisy => "## air travel itinerary receipt",
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
        SyntheticVariant::Noisy => "### flight segments",
    };
    let traveler_label = if document_id.ends_with('1') || document_id.ends_with('4') {
        "Passenger Name"
    } else {
        "Traveler Name"
    };
    let confirmation_label = if document_id.ends_with('2') || document_id.ends_with('5') {
        "Record Locator"
    } else {
        "Booking Reference"
    };
    let ticket_label = if document_id.ends_with('3') { "Ticket No" } else { "Ticket Number" };
    let booking_label = if document_id.ends_with('6') { "Issue Date" } else { "Booking Date" };
    let origin_label = if document_id.ends_with('7') { "From" } else { "Origin" };
    let destination_label = if document_id.ends_with('8') { "To" } else { "Destination" };
    let total_label = if document_id.ends_with('9') { "Fare Paid" } else { "Total Paid" };
    let traveler_line = format!("{bullet} {traveler_label}: {traveler}");
    let confirmation_code = format!("{}{}{}", carrier.outbound_flight_number.chars().next().unwrap_or('A'), destination.airport_code.chars().next().unwrap_or('A'), packet_id.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>());
    let confirmation_line = format!("{bullet} {confirmation_label}: {confirmation_code}");
    let ticket_number = format!(
        "016{:010}",
        3_100_000_000u64 + (packet_id.bytes().fold(0u64, |acc, byte| acc + byte as u64) % 9_000_000_000u64)
    );
    let ticket_line = format!("{bullet} {ticket_label}: {ticket_number}");
    let booking_line = format!("{bullet} {booking_label}: {}", schedule.booking_date);
    let origin_line = format!("{bullet} {origin_label}: San Francisco, CA, United States (SFO)");
    let destination_line = format!(
        "{bullet} {destination_label}: {}, {} ({})",
        destination.city, destination.country, destination.airport_code
    );
    let trip_window_line = format!(
        "{bullet} Trip Window: {} to {}",
        schedule.departure_date, schedule.return_date
    );
    let departure_line = format!("{bullet} Departure: {}", schedule.departure_date);
    let return_line = format!("{bullet} Return: {}", schedule.return_date);
    let total_paid_line = format!(
        "{bullet} {total_label}: USD {}",
        amount_to_string(airfare_total_cents)
    );
    let segment_one_line = format!(
        "{bullet} Segment 1 | Departure Airport: SFO | Arrival Airport: {} | Departure Date: {} | Arrival Date: {} | Marketing Carrier: {} | Flight Number: {} | Cabin Class: {}",
        destination.airport_code,
        schedule.departure_date,
        schedule.outbound_arrival_date,
        carrier.marketing_carrier,
        carrier.outbound_flight_number,
        carrier.cabin_class,
    );
    let segment_two_line = format!(
        "{bullet} Segment 2 | Departure Airport: {} | Arrival Airport: SFO | Departure Date: {} | Arrival Date: {} | Marketing Carrier: {} | Flight Number: {} | Cabin Class: {}",
        destination.airport_code,
        schedule.return_date,
        schedule.return_date,
        carrier.marketing_carrier,
        carrier.inbound_flight_number,
        carrier.cabin_class,
    );

    let mut markdown_lines = vec![
        heading.to_owned(),
        String::new(),
        passenger_heading.to_owned(),
        traveler_line.clone(),
        confirmation_line.clone(),
        ticket_line.clone(),
        booking_line.clone(),
        String::new(),
        trip_heading.to_owned(),
        origin_line.clone(),
        destination_line.clone(),
    ];
    match trip_window_mode {
        TripWindowMode::Explicit => markdown_lines.push(trip_window_line.clone()),
        TripWindowMode::DepartureReturnDerived => {
            markdown_lines.push(departure_line.clone());
            markdown_lines.push(return_line.clone());
        }
        TripWindowMode::SegmentDerived => {}
    }
    markdown_lines.push(total_paid_line.clone());
    markdown_lines.push(String::new());
    markdown_lines.push(segment_heading.to_owned());
    markdown_lines.push(segment_one_line.clone());
    markdown_lines.push(segment_two_line.clone());
    markdown_lines.push(String::new());
    let markdown = markdown_lines.join("\n");

    let trip_window = match trip_window_mode {
        TripWindowMode::Explicit => Some(observed(
            DateRange {
                start_date: schedule.departure_date.to_owned(),
                end_date: schedule.return_date.to_owned(),
            },
            &document_id,
            &filename,
            &trip_window_line,
        )),
        TripWindowMode::DepartureReturnDerived => Some(inferred_date_range(
            &document_id,
            &filename,
            schedule.departure_date,
            schedule.return_date,
            &[departure_line.as_str(), return_line.as_str()],
            "document_extract.infer_trip_window_from_departure_return",
        )),
        TripWindowMode::SegmentDerived => Some(inferred_date_range(
            &document_id,
            &filename,
            schedule.departure_date,
            schedule.return_date,
            &[segment_one_line.as_str(), segment_two_line.as_str()],
            "document_extract.infer_trip_window_from_segments",
        )),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::FlightItinerary,
        filename: filename.clone(),
        markdown,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.clone(),
            filename: filename.clone(),
            classification: classification(DocumentKind::FlightItinerary, &document_id, &filename, heading),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::FlightItinerary(FlightItineraryFacts {
                traveler_names: vec![observed(
                    traveler.to_owned(),
                    &document_id,
                    &filename,
                    &traveler_line,
                )],
                confirmation_code: Some(observed(
                    confirmation_code,
                    &document_id,
                    &filename,
                    &confirmation_line,
                )),
                ticket_number: Some(observed(
                    ticket_number,
                    &document_id,
                    &filename,
                    &ticket_line,
                )),
                booking_date: Some(observed(
                    schedule.booking_date.to_owned(),
                    &document_id,
                    &filename,
                    &booking_line,
                )),
                trip_window,
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
                        city: Some(destination.city.to_owned()),
                        region: None,
                        country: Some(destination.country.to_owned()),
                        airport_code: Some(destination.airport_code.to_owned()),
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
                            destination.airport_code.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        ),
                        departure_date: observed(
                            schedule.departure_date.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        ),
                        arrival_date: Some(observed(
                            schedule.outbound_arrival_date.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        )),
                        marketing_carrier: Some(observed(
                            carrier.marketing_carrier.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        )),
                        flight_number: Some(observed(
                            carrier.outbound_flight_number.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        )),
                        cabin_class: Some(observed(
                            carrier.cabin_class.to_owned(),
                            &document_id,
                            &filename,
                            &segment_one_line,
                        )),
                    },
                    FlightSegmentFacts {
                        departure_airport: observed(
                            destination.airport_code.to_owned(),
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
                            schedule.return_date.to_owned(),
                            &document_id,
                            &filename,
                            &segment_two_line,
                        ),
                        arrival_date: Some(observed(
                            schedule.return_date.to_owned(),
                            &document_id,
                            &filename,
                            &segment_two_line,
                        )),
                        marketing_carrier: Some(observed(
                            carrier.marketing_carrier.to_owned(),
                            &document_id,
                            &filename,
                            &segment_two_line,
                        )),
                        flight_number: Some(observed(
                            carrier.inbound_flight_number.to_owned(),
                            &document_id,
                            &filename,
                            &segment_two_line,
                        )),
                        cabin_class: Some(observed(
                            carrier.cabin_class.to_owned(),
                            &document_id,
                            &filename,
                            &segment_two_line,
                        )),
                    },
                ],
                total_paid: Some(observed(
                    MoneyAmount {
                        amount: amount_to_string(airfare_total_cents),
                        currency: Some("USD".to_owned()),
                    },
                    &document_id,
                    &filename,
                    &total_paid_line,
                )),
            }),
            issues: Vec::new(),
        },
    }
}

fn build_hotel_fixture(
    packet_id: &str,
    traveler: &str,
    destination: &DestinationProfile,
    schedule: &ScheduleProfile,
    variant: SyntheticVariant,
    stay_window_mode: StayWindowMode,
    hotel_total_mode: TotalMode,
    room_rate_cents: i64,
    hotel_tax_cents: i64,
    hotel_total_cents: i64,
) -> SyntheticDocumentFixture {
    let filename = format!("{packet_id}_hotel_folio.md");
    let document_id = document_id_for_filename(&filename);
    let heading = match variant {
        SyntheticVariant::Baseline => "# Hotel Folio",
        SyntheticVariant::Noisy => "## guest bill",
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
        SyntheticVariant::Noisy => "### charges",
    };
    let meals_heading = match variant {
        SyntheticVariant::Baseline => "## Meals Included",
        SyntheticVariant::Noisy => "### included in room rate",
    };
    let property_label = if document_id.ends_with('2') { "Hotel Name" } else { "Property Name" };
    let guest_label = if document_id.ends_with('3') { "Guest" } else { "Guest Name" };
    let folio_label = if document_id.ends_with('4') { "Folio #" } else { "Folio Number" };
    let location_label = if document_id.ends_with('5') { "City/Country" } else { "Property Location" };
    let property_name = destination.hotels[packet_id.bytes().fold(0usize, |acc, byte| acc + byte as usize) % destination.hotels.len()];
    let room_description = destination.room_descriptions
        [packet_id.bytes().fold(0usize, |acc, byte| acc + (byte as usize * 3)) % destination.room_descriptions.len()];
    let property_line = format!("{bullet} {property_label}: {property_name}");
    let guest_line = format!("{bullet} {guest_label}: {traveler}");
    let folio_line = format!(
        "{bullet} {folio_label}: {}-{}",
        &destination.airport_code[..2],
        84_000 + (packet_id.bytes().fold(0usize, |acc, byte| acc + byte as usize) % 900)
    );
    let location_line = format!(
        "{bullet} {location_label}: {}, {}",
        destination.city, destination.country
    );
    let stay_window_line = format!(
        "{bullet} Stay Window: {} to {}",
        schedule.hotel_nights[0], schedule.checkout_date
    );
    let check_in_line = format!("{bullet} Check In: {}", schedule.hotel_nights[0]);
    let check_out_line = format!("{bullet} Check Out: {}", schedule.checkout_date);
    let total_line = format!(
        "{bullet} Total Paid: {} {}",
        destination.currency,
        amount_to_string(hotel_total_cents)
    );
    let night_lines = schedule
        .hotel_nights
        .iter()
        .map(|date| {
            format!(
                "{bullet} Date: {date} | Description: {room_description} | Room Rate: {} {} | Taxes & Fees: {} {}",
                destination.currency,
                amount_to_string(room_rate_cents),
                destination.currency,
                amount_to_string(hotel_tax_cents),
            )
        })
        .collect::<Vec<_>>();
    let meal_one_line = format!("{bullet} Breakfast");
    let meal_two_line = format!("{bullet} Evening Reception");

    let mut markdown_lines = vec![
        heading.to_owned(),
        String::new(),
        stay_heading.to_owned(),
        property_line.clone(),
        guest_line.clone(),
        folio_line.clone(),
        location_line.clone(),
    ];
    match stay_window_mode {
        StayWindowMode::Explicit => markdown_lines.push(stay_window_line.clone()),
        StayWindowMode::CheckInOutDerived => {
            markdown_lines.push(check_in_line.clone());
            markdown_lines.push(check_out_line.clone());
        }
        StayWindowMode::NightDerived => {}
    }
    if hotel_total_mode == TotalMode::Explicit {
        markdown_lines.push(total_line.clone());
    }
    markdown_lines.push(String::new());
    markdown_lines.push(nightly_heading.to_owned());
    markdown_lines.extend(night_lines.iter().cloned());
    markdown_lines.push(String::new());
    markdown_lines.push(meals_heading.to_owned());
    markdown_lines.push(meal_one_line.clone());
    markdown_lines.push(meal_two_line.clone());
    markdown_lines.push(String::new());
    let markdown = markdown_lines.join("\n");

    let stay_window = match stay_window_mode {
        StayWindowMode::Explicit => Some(observed(
            DateRange {
                start_date: schedule.hotel_nights[0].to_owned(),
                end_date: schedule.checkout_date.to_owned(),
            },
            &document_id,
            &filename,
            &stay_window_line,
        )),
        StayWindowMode::CheckInOutDerived => Some(inferred_date_range(
            &document_id,
            &filename,
            schedule.hotel_nights[0],
            schedule.checkout_date,
            &[check_in_line.as_str(), check_out_line.as_str()],
            "document_extract.infer_stay_window_from_checkin_checkout",
        )),
        StayWindowMode::NightDerived => Some(inferred_date_range(
            &document_id,
            &filename,
            schedule.hotel_nights[0],
            schedule.hotel_nights[2],
            &[night_lines[0].as_str(), night_lines[2].as_str()],
            "document_extract.infer_stay_window_from_nights",
        )),
    };
    let total_paid = match hotel_total_mode {
        TotalMode::Explicit => Some(observed(
            MoneyAmount {
                amount: amount_to_string(hotel_total_cents),
                currency: Some(destination.currency.to_owned()),
            },
            &document_id,
            &filename,
            &total_line,
        )),
        TotalMode::Derived => Some(inferred_hotel_total_from_nights(
            &document_id,
            &filename,
            hotel_total_cents,
            destination.currency,
            &night_lines,
        )),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::HotelFolio,
        filename: filename.clone(),
        markdown,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.clone(),
            filename: filename.clone(),
            classification: classification(DocumentKind::HotelFolio, &document_id, &filename, heading),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::HotelFolio(HotelFolioFacts {
                guest_name: Some(observed(
                    traveler.to_owned(),
                    &document_id,
                    &filename,
                    &guest_line,
                )),
                property_name: Some(observed(
                    property_name.to_owned(),
                    &document_id,
                    &filename,
                    &property_line,
                )),
                folio_number: Some(observed(
                    folio_line
                        .split(':')
                        .nth(1)
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_owned(),
                    &document_id,
                    &filename,
                    &folio_line,
                )),
                stay_window,
                property_location: Some(observed(
                    Location {
                        city: Some(destination.city.to_owned()),
                        region: None,
                        country: Some(destination.country.to_owned()),
                        airport_code: None,
                    },
                    &document_id,
                    &filename,
                    &location_line,
                )),
                nightly_charges: night_lines
                    .iter()
                    .zip(schedule.hotel_nights.iter())
                    .map(|(quote, date)| HotelNightChargeFacts {
                        date: observed(date.to_string(), &document_id, &filename, quote),
                        room_rate: Some(observed(
                            MoneyAmount {
                                amount: amount_to_string(room_rate_cents),
                                currency: Some(destination.currency.to_owned()),
                            },
                            &document_id,
                            &filename,
                            quote,
                        )),
                        taxes_and_fees: vec![observed(
                            MoneyAmount {
                                amount: amount_to_string(hotel_tax_cents),
                                currency: Some(destination.currency.to_owned()),
                            },
                            &document_id,
                            &filename,
                            quote,
                        )],
                        description: Some(observed(
                            room_description.to_owned(),
                            &document_id,
                            &filename,
                            quote,
                        )),
                    })
                    .collect(),
                total_paid,
                meals_included: vec![
                    observed("Breakfast".to_owned(), &document_id, &filename, &meal_one_line),
                    observed(
                        "Evening Reception".to_owned(),
                        &document_id,
                        &filename,
                        &meal_two_line,
                    ),
                ],
            }),
            issues: Vec::new(),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn build_receipt_fixture(
    packet_id: &str,
    destination: &DestinationProfile,
    schedule: &ScheduleProfile,
    variant: SyntheticVariant,
    receipt_total_mode: TotalMode,
    subtotal_cents: i64,
    tax_cents: i64,
    tip_cents: i64,
    total_receipt_cents: i64,
    first_item_cents: i64,
    second_item_cents: i64,
    third_item_cents: i64,
    alcohol_item_cents: i64,
    alcohol_receipt: bool,
) -> SyntheticDocumentFixture {
    let filename = format!("{packet_id}_receipt.md");
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
        SyntheticVariant::Noisy => "### items",
    };
    let merchant_label = if document_id.ends_with('2') { "Merchant" } else { "Merchant Name" };
    let location_label = if document_id.ends_with('4') { "Location" } else { "Merchant Location" };
    let date_label = if document_id.ends_with('6') { "Date" } else { "Transaction Date" };
    let total_label = if document_id.ends_with('8') { "Amount Paid" } else { "Total Paid" };
    let merchant_name = destination.merchants
        [packet_id.bytes().fold(0usize, |acc, byte| acc + byte as usize) % destination.merchants.len()];
    let merchant_line = format!("{bullet} {merchant_label}: {merchant_name}");
    let location_line = format!(
        "{bullet} {location_label}: {}, {}",
        destination.city, destination.country
    );
    let date_line = format!("{bullet} {date_label}: {}", schedule.receipt_date);
    let subtotal_line = format!(
        "{bullet} Subtotal: {} {}",
        destination.currency,
        amount_to_string(subtotal_cents)
    );
    let tax_line = format!(
        "{bullet} Tax: {} {}",
        destination.currency,
        amount_to_string(tax_cents)
    );
    let tip_line = format!(
        "{bullet} Tip: {} {}",
        destination.currency,
        amount_to_string(tip_cents)
    );
    let total_line = format!(
        "{bullet} {total_label}: {} {}",
        destination.currency,
        amount_to_string(total_receipt_cents)
    );
    let item_one_line = format!(
        "{bullet} {} | {} {}",
        destination.line_items[0],
        destination.currency,
        amount_to_string(first_item_cents)
    );
    let item_two_line = format!(
        "{bullet} {} | {} {}",
        destination.line_items[1],
        destination.currency,
        amount_to_string(second_item_cents)
    );
    let item_three_label = if alcohol_receipt {
        "Wine Pairing"
    } else {
        destination.line_items[2]
    };
    let item_three_line = format!(
        "{bullet} {item_three_label} | {} {}",
        destination.currency,
        amount_to_string(if alcohol_receipt {
            alcohol_item_cents
        } else {
            third_item_cents
        })
    );
    let service_charge_line = if alcohol_receipt {
        Some(format!(
            "{bullet} {} | {} {}",
            destination.line_items[2],
            destination.currency,
            amount_to_string(third_item_cents)
        ))
    } else {
        None
    };

    let mut markdown_lines = vec![
        heading.to_owned(),
        String::new(),
        purchase_heading.to_owned(),
        merchant_line.clone(),
        location_line.clone(),
        date_line.clone(),
        subtotal_line.clone(),
        tax_line.clone(),
        tip_line.clone(),
    ];
    if receipt_total_mode == TotalMode::Explicit {
        markdown_lines.push(total_line.clone());
    }
    markdown_lines.push(String::new());
    markdown_lines.push(line_items_heading.to_owned());
    markdown_lines.push(item_one_line.clone());
    markdown_lines.push(item_two_line.clone());
    markdown_lines.push(item_three_line.clone());
    if let Some(service_charge_line) = service_charge_line.as_ref() {
        markdown_lines.push(service_charge_line.clone());
    }
    markdown_lines.push(String::new());
    let markdown = markdown_lines.join("\n");

    let mut line_items = vec![
        ReceiptLineItemFacts {
            description: observed(
                destination.line_items[0].to_owned(),
                &document_id,
                &filename,
                &item_one_line,
            ),
            amount: observed(
                MoneyAmount {
                    amount: amount_to_string(first_item_cents),
                    currency: Some(destination.currency.to_owned()),
                },
                &document_id,
                &filename,
                &item_one_line,
            ),
        },
        ReceiptLineItemFacts {
            description: observed(
                destination.line_items[1].to_owned(),
                &document_id,
                &filename,
                &item_two_line,
            ),
            amount: observed(
                MoneyAmount {
                    amount: amount_to_string(second_item_cents),
                    currency: Some(destination.currency.to_owned()),
                },
                &document_id,
                &filename,
                &item_two_line,
            ),
        },
        ReceiptLineItemFacts {
            description: observed(
                item_three_label.to_owned(),
                &document_id,
                &filename,
                &item_three_line,
            ),
            amount: observed(
                MoneyAmount {
                    amount: amount_to_string(if alcohol_receipt {
                        alcohol_item_cents
                    } else {
                        third_item_cents
                    }),
                    currency: Some(destination.currency.to_owned()),
                },
                &document_id,
                &filename,
                &item_three_line,
            ),
        },
    ];
    if let Some(service_charge_line) = service_charge_line.as_ref() {
        line_items.push(ReceiptLineItemFacts {
            description: observed(
                destination.line_items[2].to_owned(),
                &document_id,
                &filename,
                service_charge_line,
            ),
            amount: observed(
                MoneyAmount {
                    amount: amount_to_string(third_item_cents),
                    currency: Some(destination.currency.to_owned()),
                },
                &document_id,
                &filename,
                service_charge_line,
            ),
        });
    }

    let total_paid = match receipt_total_mode {
        TotalMode::Explicit => Some(observed(
            MoneyAmount {
                amount: amount_to_string(total_receipt_cents),
                currency: Some(destination.currency.to_owned()),
            },
            &document_id,
            &filename,
            &total_line,
        )),
        TotalMode::Derived => Some(inferred_money_from_quotes(
            &document_id,
            &filename,
            total_receipt_cents,
            destination.currency,
            &[subtotal_line.clone(), tax_line.clone(), tip_line.clone()],
            "document_extract.infer_receipt_total",
        )),
    };

    SyntheticDocumentFixture {
        kind: DocumentKind::Receipt,
        filename: filename.clone(),
        markdown,
        expected_facts: ExtractedDocumentFacts {
            document_id: document_id.clone(),
            filename: filename.clone(),
            classification: classification(DocumentKind::Receipt, &document_id, &filename, heading),
            extraction_status: ExtractionStatus::Complete,
            facts: DocumentFactsPayload::Receipt(ReceiptFacts {
                merchant_name: Some(observed(
                    merchant_name.to_owned(),
                    &document_id,
                    &filename,
                    &merchant_line,
                )),
                merchant_location: Some(observed(
                    Location {
                        city: Some(destination.city.to_owned()),
                        region: None,
                        country: Some(destination.country.to_owned()),
                        airport_code: None,
                    },
                    &document_id,
                    &filename,
                    &location_line,
                )),
                transaction_date: Some(observed(
                    schedule.receipt_date.to_owned(),
                    &document_id,
                    &filename,
                    &date_line,
                )),
                total_paid,
                subtotal: Some(observed(
                    MoneyAmount {
                        amount: amount_to_string(subtotal_cents),
                        currency: Some(destination.currency.to_owned()),
                    },
                    &document_id,
                    &filename,
                    &subtotal_line,
                )),
                tax_amount: Some(observed(
                    MoneyAmount {
                        amount: amount_to_string(tax_cents),
                        currency: Some(destination.currency.to_owned()),
                    },
                    &document_id,
                    &filename,
                    &tax_line,
                )),
                tip_amount: Some(observed(
                    MoneyAmount {
                        amount: amount_to_string(tip_cents),
                        currency: Some(destination.currency.to_owned()),
                    },
                    &document_id,
                    &filename,
                    &tip_line,
                )),
                line_items,
            }),
            issues: Vec::new(),
        },
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

fn inferred_date_range(
    document_id: &str,
    filename: &str,
    start_date: &str,
    end_date: &str,
    quotes: &[&str],
    origin: &str,
) -> Observed<DateRange> {
    let mut evidence = quotes
        .iter()
        .map(|quote| document_span(document_id, filename, quote))
        .collect::<Vec<_>>();
    evidence.push(system_generated(origin));
    Observed {
        value: DateRange {
            start_date: start_date.to_owned(),
            end_date: end_date.to_owned(),
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    }
}

fn inferred_money_from_quotes(
    document_id: &str,
    filename: &str,
    cents: i64,
    currency: &str,
    quotes: &[impl AsRef<str>],
    origin: &str,
) -> Observed<MoneyAmount> {
    let mut evidence = quotes
        .iter()
        .map(|quote| document_span(document_id, filename, quote.as_ref()))
        .collect::<Vec<_>>();
    evidence.push(system_generated(origin));
    Observed {
        value: MoneyAmount {
            amount: amount_to_string(cents),
            currency: Some(currency.to_owned()),
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    }
}

fn inferred_hotel_total_from_nights(
    document_id: &str,
    filename: &str,
    cents: i64,
    currency: &str,
    night_quotes: &[String],
) -> Observed<MoneyAmount> {
    let mut evidence = Vec::new();
    for quote in night_quotes {
        evidence.push(document_span(document_id, filename, quote));
        evidence.push(document_span(document_id, filename, quote));
    }
    evidence.push(system_generated(
        "document_extract.infer_hotel_total_from_nights",
    ));
    Observed {
        value: MoneyAmount {
            amount: amount_to_string(cents),
            currency: Some(currency.to_owned()),
        },
        confidence: ConfidenceLevel::Medium,
        evidence,
        flags: Vec::new(),
    }
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

fn system_generated(origin: &str) -> EvidenceReference {
    EvidenceReference {
        kind: EvidenceKind::SystemGenerated,
        document_id: None,
        filename: None,
        page: None,
        quote: None,
        origin: Some(origin.to_owned()),
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

fn amount_to_string(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        generate_synthetic_corpus, StayWindowMode, SyntheticLedgerScenario, TotalMode,
        TripWindowMode,
    };

    #[test]
    fn corpus_packet_ids_are_unique() {
        let mut ids = BTreeSet::new();
        for packet in generate_synthetic_corpus(48) {
            assert!(ids.insert(packet.packet_id.clone()));
            assert_eq!(packet.fixtures.len(), 3);
        }
    }

    #[test]
    fn corpus_spans_multiple_modes_and_scenarios() {
        let packets = generate_synthetic_corpus(36);
        assert!(packets
            .iter()
            .any(|packet| packet.scenario == SyntheticLedgerScenario::Accepted));
        assert!(packets
            .iter()
            .any(|packet| packet.scenario == SyntheticLedgerScenario::ReturnedAndCorrected));
        assert!(packets
            .iter()
            .any(|packet| packet.trip_window_mode == TripWindowMode::SegmentDerived));
        assert!(packets
            .iter()
            .any(|packet| packet.stay_window_mode == StayWindowMode::NightDerived));
        assert!(packets
            .iter()
            .any(|packet| packet.receipt_total_mode == TotalMode::Derived));
    }
}
