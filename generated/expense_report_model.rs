// Auto-generated typed model from schema.yaml. Do not edit manually.

use serde::{Deserialize, Serialize};

use crate::meta::Wrapped;

pub const SCHEMA_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IsoDate(pub String);

impl From<String> for IsoDate {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for IsoDate {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Source tier: T3
/// Infer from:  Destination in flight/hotel docs. Foreign destination → expenses_foreign;
/// Infer from: domestic → expenses_domestic. Other categories require explicit context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportGeneralInformationCategoryEnum {
    #[default]
    ExpensesDomestic,
    ExpensesForeign,
    AthleticUseOnly,
    HrUseOnly,
    HumanSubjects,
    Relocation,
}

impl ExpenseReportGeneralInformationCategoryEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ExpensesDomestic => "expenses_domestic",
            Self::ExpensesForeign => "expenses_foreign",
            Self::AthleticUseOnly => "athletic_use_only",
            Self::HrUseOnly => "hr_use_only",
            Self::HumanSubjects => "human_subjects",
            Self::Relocation => "relocation",
        }
    }
}

impl core::str::FromStr for ExpenseReportGeneralInformationCategoryEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "expenses_domestic" => Ok(Self::ExpensesDomestic),
            "expenses_foreign" => Ok(Self::ExpensesForeign),
            "athletic_use_only" => Ok(Self::AthleticUseOnly),
            "hr_use_only" => Ok(Self::HrUseOnly),
            "human_subjects" => Ok(Self::HumanSubjects),
            "relocation" => Ok(Self::Relocation),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportGeneralInformationCategoryEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Context from uploaded docs or FA input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportGeneralInformationPayeeAffiliationEnum {
    #[default]
    StanfordStudent,
    StanfordPostdoc,
    StanfordFaculty,
    StanfordStaff,
    Other,
}

impl ExpenseReportGeneralInformationPayeeAffiliationEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::StanfordStudent => "stanford_student",
            Self::StanfordPostdoc => "stanford_postdoc",
            Self::StanfordFaculty => "stanford_faculty",
            Self::StanfordStaff => "stanford_staff",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportGeneralInformationPayeeAffiliationEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "stanford_student" => Ok(Self::StanfordStudent),
            "stanford_postdoc" => Ok(Self::StanfordPostdoc),
            "stanford_faculty" => Ok(Self::StanfordFaculty),
            "stanford_staff" => Ok(Self::StanfordStaff),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportGeneralInformationPayeeAffiliationEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

///  Defaults to 'no' unless explicitly requested
/// Source tier: T1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportGeneralInformationRushProcessingEnum {
    #[default]
    Yes,
    No,
}

impl ExpenseReportGeneralInformationRushProcessingEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}

impl core::str::FromStr for ExpenseReportGeneralInformationRushProcessingEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportGeneralInformationRushProcessingEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

///  FA-entered submission state.
/// Source tier: T1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionSummaryStatusEnum {
    #[default]
    Draft,
    Submitted,
    Approved,
    Returned,
    Paid,
}

impl ExpenseReportTransactionSummaryStatusEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::Approved => "approved",
            Self::Returned => "returned",
            Self::Paid => "paid",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionSummaryStatusEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "submitted" => Ok(Self::Submitted),
            "approved" => Ok(Self::Approved),
            "returned" => Ok(Self::Returned),
            "paid" => Ok(Self::Paid),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionSummaryStatusEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  LLM classifies from receipt content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemCommonExpenseTypeEnum {
    #[default]
    AdjustedPerDiem,
    AirfareDomestic,
    AirfareForeign,
    AncillaryAirlineFee,
    BusinessMeal,
    CarRental,
    ConferenceRegistration,
    GiftCardEmployeeForeign,
    GiftsForeignActivity,
    GroundTransportationForeign,
    GroundTransportationDomestic,
    GroupTravelMeal,
    HumanSubjectIncentive,
    LodgingDomestic,
    LodgingForeign,
    OtherBusinessExpense,
}

impl ExpenseReportTransactionLinesItemCommonExpenseTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AdjustedPerDiem => "adjusted_per_diem",
            Self::AirfareDomestic => "airfare_domestic",
            Self::AirfareForeign => "airfare_foreign",
            Self::AncillaryAirlineFee => "ancillary_airline_fee",
            Self::BusinessMeal => "business_meal",
            Self::CarRental => "car_rental",
            Self::ConferenceRegistration => "conference_registration",
            Self::GiftCardEmployeeForeign => "gift_card_employee_foreign",
            Self::GiftsForeignActivity => "gifts_foreign_activity",
            Self::GroundTransportationForeign => "ground_transportation_foreign",
            Self::GroundTransportationDomestic => "ground_transportation_domestic",
            Self::GroupTravelMeal => "group_travel_meal",
            Self::HumanSubjectIncentive => "human_subject_incentive",
            Self::LodgingDomestic => "lodging_domestic",
            Self::LodgingForeign => "lodging_foreign",
            Self::OtherBusinessExpense => "other_business_expense",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemCommonExpenseTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "adjusted_per_diem" => Ok(Self::AdjustedPerDiem),
            "airfare_domestic" => Ok(Self::AirfareDomestic),
            "airfare_foreign" => Ok(Self::AirfareForeign),
            "ancillary_airline_fee" => Ok(Self::AncillaryAirlineFee),
            "business_meal" => Ok(Self::BusinessMeal),
            "car_rental" => Ok(Self::CarRental),
            "conference_registration" => Ok(Self::ConferenceRegistration),
            "gift_card_employee_foreign" => Ok(Self::GiftCardEmployeeForeign),
            "gifts_foreign_activity" => Ok(Self::GiftsForeignActivity),
            "ground_transportation_foreign" => Ok(Self::GroundTransportationForeign),
            "ground_transportation_domestic" => Ok(Self::GroundTransportationDomestic),
            "group_travel_meal" => Ok(Self::GroupTravelMeal),
            "human_subject_incentive" => Ok(Self::HumanSubjectIncentive),
            "lodging_domestic" => Ok(Self::LodgingDomestic),
            "lodging_foreign" => Ok(Self::LodgingForeign),
            "other_business_expense" => Ok(Self::OtherBusinessExpense),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemCommonExpenseTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Conditionally required when:  expense_type in [airfare_foreign, gift_card_employee_foreign,
/// Conditionally required when: gifts_foreign_activity, ground_transportation_foreign,
/// Conditionally required when: lodging_foreign]
/// Source tier: T3
/// Infer from:  Conference registration → 'conference'; otherwise from context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum {
    #[default]
    Conference,
    ResearchCollaboration,
    Fieldwork,
    Other,
}

impl ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Conference => "conference",
            Self::ResearchCollaboration => "research_collaboration",
            Self::Fieldwork => "fieldwork",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conference" => Ok(Self::Conference),
            "research_collaboration" => Ok(Self::ResearchCollaboration),
            "fieldwork" => Ok(Self::Fieldwork),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

///  Document kind chosen by the FA at upload time.
/// Source tier: T1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum {
    #[default]
    Receipt,
    BookingConfirmation,
    PriceComparison,
    CurrencyConversion,
    ConferenceProgram,
    MissingReceiptForm,
    Other,
}

impl ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Receipt => "receipt",
            Self::BookingConfirmation => "booking_confirmation",
            Self::PriceComparison => "price_comparison",
            Self::CurrencyConversion => "currency_conversion",
            Self::ConferenceProgram => "conference_program",
            Self::MissingReceiptForm => "missing_receipt_form",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "receipt" => Ok(Self::Receipt),
            "booking_confirmation" => Ok(Self::BookingConfirmation),
            "price_comparison" => Ok(Self::PriceComparison),
            "currency_conversion" => Ok(Self::CurrencyConversion),
            "conference_program" => Ok(Self::ConferenceProgram),
            "missing_receipt_form" => Ok(Self::MissingReceiptForm),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Booking confirmation format/header; default 'other' if unrecognized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum {
    #[default]
    StanfordTravelEgencia,
    StanfordTravelKeyTravel,
    StanfordTravelConnectUa,
    StanfordTravelConnectDl,
    StanfordTravelConnectAa,
    StanfordTravelConnectAs,
    StanfordTravelConnectHa,
    Other,
}

impl ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::StanfordTravelEgencia => "stanford_travel_egencia",
            Self::StanfordTravelKeyTravel => "stanford_travel_key_travel",
            Self::StanfordTravelConnectUa => "stanford_travel_connect_ua",
            Self::StanfordTravelConnectDl => "stanford_travel_connect_dl",
            Self::StanfordTravelConnectAa => "stanford_travel_connect_aa",
            Self::StanfordTravelConnectAs => "stanford_travel_connect_as",
            Self::StanfordTravelConnectHa => "stanford_travel_connect_ha",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "stanford_travel_egencia" => Ok(Self::StanfordTravelEgencia),
            "stanford_travel_key_travel" => Ok(Self::StanfordTravelKeyTravel),
            "stanford_travel_connect_ua" => Ok(Self::StanfordTravelConnectUa),
            "stanford_travel_connect_dl" => Ok(Self::StanfordTravelConnectDl),
            "stanford_travel_connect_aa" => Ok(Self::StanfordTravelConnectAa),
            "stanford_travel_connect_as" => Ok(Self::StanfordTravelConnectAs),
            "stanford_travel_connect_ha" => Ok(Self::StanfordTravelConnectHa),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Booking confirmation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum {
    #[default]
    Coach,
    PremiumEconomy,
    Business,
    First,
}

impl ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Coach => "coach",
            Self::PremiumEconomy => "premium_economy",
            Self::Business => "business",
            Self::First => "first",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "coach" => Ok(Self::Coach),
            "premium_economy" => Ok(Self::PremiumEconomy),
            "business" => Ok(Self::Business),
            "first" => Ok(Self::First),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  If hotel matches conference venue → conference_hotel; else check booking source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum {
    #[default]
    ConferenceHotel,
    StanfordTravelEgencia,
    StanfordTravelKeyTravel,
    Other,
}

impl ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ConferenceHotel => "conference_hotel",
            Self::StanfordTravelEgencia => "stanford_travel_egencia",
            Self::StanfordTravelKeyTravel => "stanford_travel_key_travel",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conference_hotel" => Ok(Self::ConferenceHotel),
            "stanford_travel_egencia" => Ok(Self::StanfordTravelEgencia),
            "stanford_travel_key_travel" => Ok(Self::StanfordTravelKeyTravel),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Registration receipt format/branding (Whova logo, ACM portal style, etc.);
/// Infer from: default 'other' if unrecognized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportTransactionLinesItemConferenceRegistrationDetailsRegistrationSystemEnum {
    #[default]
    Whova,
    Cvent,
    AcmRegonline,
    Eventbrite,
    Usenix,
    Other,
}

impl ExpenseReportTransactionLinesItemConferenceRegistrationDetailsRegistrationSystemEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Whova => "whova",
            Self::Cvent => "cvent",
            Self::AcmRegonline => "acm_regonline",
            Self::Eventbrite => "eventbrite",
            Self::Usenix => "usenix",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemConferenceRegistrationDetailsRegistrationSystemEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "whova" => Ok(Self::Whova),
            "cvent" => Ok(Self::Cvent),
            "acm_regonline" => Ok(Self::AcmRegonline),
            "eventbrite" => Ok(Self::Eventbrite),
            "usenix" => Ok(Self::Usenix),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemConferenceRegistrationDetailsRegistrationSystemEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Destination from flight/hotel docs determines domestic vs. international;
/// Infer from: location determines AK/HI vs. continental
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportPerDiemExpensesItemExpenseTypeEnum {
    #[default]
    AlaskaHawaiiLodging,
    AlaskaHawaiiMeals,
    ContinentalUsLodging,
    ContinentalUsMeals,
    InternationalLodging,
    InternationalMeals,
}

impl ExpenseReportPerDiemExpensesItemExpenseTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::AlaskaHawaiiLodging => "alaska_hawaii_lodging",
            Self::AlaskaHawaiiMeals => "alaska_hawaii_meals",
            Self::ContinentalUsLodging => "continental_us_lodging",
            Self::ContinentalUsMeals => "continental_us_meals",
            Self::InternationalLodging => "international_lodging",
            Self::InternationalMeals => "international_meals",
        }
    }
}

impl core::str::FromStr for ExpenseReportPerDiemExpensesItemExpenseTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "alaska_hawaii_lodging" => Ok(Self::AlaskaHawaiiLodging),
            "alaska_hawaii_meals" => Ok(Self::AlaskaHawaiiMeals),
            "continental_us_lodging" => Ok(Self::ContinentalUsLodging),
            "continental_us_meals" => Ok(Self::ContinentalUsMeals),
            "international_lodging" => Ok(Self::InternationalLodging),
            "international_meals" => Ok(Self::InternationalMeals),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportPerDiemExpensesItemExpenseTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Conditionally required when:  expense_type in [international_lodging, international_meals]
/// Source tier: T3
/// Infer from:  Inferred from conference registration or trip context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum {
    #[default]
    Conference,
    ResearchCollaboration,
    Fieldwork,
    Other,
}

impl ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Conference => "conference",
            Self::ResearchCollaboration => "research_collaboration",
            Self::Fieldwork => "fieldwork",
            Self::Other => "other",
        }
    }
}

impl core::str::FromStr for ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "conference" => Ok(Self::Conference),
            "research_collaboration" => Ok(Self::ResearchCollaboration),
            "fieldwork" => Ok(Self::Fieldwork),
            "other" => Ok(Self::Other),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

///  Person being reimbursed
/// Source tier: T3
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportGeneralInformationPayee {
    /// Source tier: T3
    /// Infer from:  Traveler name on flight booking or hotel folio
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Context from uploaded docs or FA input
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub affiliation: Wrapped<ExpenseReportGeneralInformationPayeeAffiliationEnum>,

}

///  Structured purpose statement. Phase 5 conference flow re-tiers most sub-fields away from T1
/// — primary producer is now the conference synthesis (T4) or supporting-doc aggregation (T2)
/// when conference data is uploaded; T1 fallback when not. See docs/phase-5-design.md.
/// Source tier: T1
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportGeneralInformationBusinessPurpose {
    /// Source tier: T4
    /// Infer from:  Synthesis: participant_role + receipt's attendee_name; T1 fallback when no
    /// Infer from: conference docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub who: Wrapped<String>,
    /// Source tier: T4
    /// Infer from:  Synthesis: composed sentence about what the trip is for; T1 fallback when
    /// Infer from: no conference docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub what: Wrapped<String>,
    /// Source tier: T2
    /// Infer from:  Reduction: min/max of supporting_conference_doc.scheduled_dates; T1
    /// Infer from: fallback when no supporting docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub when: Wrapped<String>,
    /// Source tier: T2
    /// Infer from:  Reduction: union of supporting_conference_doc.venues_mentioned; T1 fallback
    /// Infer from: when no supporting docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub r#where: Wrapped<String>,
    /// Source tier: T4
    /// Infer from:  Synthesis: composed sentence — presenting / collaboration / etc.; T1
    /// Infer from: fallback when no conference docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub why: Wrapped<String>,
    ///  30-character lookup key. Reduction derives from event_name + earliest scheduled date
    /// (e.g. 'ASPLOS-2026'); T1 fallback when neither is available.
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub key_30char: Wrapped<String>,

}

///  At least one reason must be selected. Determines required approvals.
/// Depends on:  general_information.payee.affiliation
/// Source tier: T3
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportGeneralInformationStudentCertification {
    ///  Requires faculty approval
    /// Source tier: T3
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub supports_faculty_research: Wrapped<bool>,
    ///  Requires faculty approval + conference program attachment
    /// Source tier: T3
    /// Infer from:  Payee name appears in conference program/agenda
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub presenting_at_conference: Wrapped<bool>,
    ///  Requires faculty approval. Not applicable to post-docs.
    /// Depends on:  general_information.payee.affiliation
    /// Source tier: T3
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub integral_to_degree_work: Wrapped<bool>,
    /// Source tier: T3
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub related_to_employment: Wrapped<bool>,
    /// Source tier: T3
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub other: Wrapped<bool>,
    /// Conditionally required when:  student_certification.other == true
    /// Source tier: T3
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub other_explanation: Wrapped<String>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportGeneralInformation {
    /// Source tier: T3
    /// Infer from:  Destination in flight/hotel docs. Foreign destination → expenses_foreign;
    /// Infer from: domestic → expenses_domestic. Other categories require explicit context.
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub category: Wrapped<ExpenseReportGeneralInformationCategoryEnum>,
    ///  Person being reimbursed
    /// Source tier: T3
    #[serde(default)]
    pub payee: ExpenseReportGeneralInformationPayee,
    ///  Defaults to 'no' unless explicitly requested
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rush_processing: Option<ExpenseReportGeneralInformationRushProcessingEnum>,
    ///  Always 'electronic' — system pre-fills
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub payment_method: Wrapped<String>,
    ///  Structured purpose statement. Phase 5 conference flow re-tiers most sub-fields away
    /// from T1 — primary producer is now the conference synthesis (T4) or supporting-doc
    /// aggregation (T2) when conference data is uploaded; T1 fallback when not. See
    /// docs/phase-5-design.md.
    /// Source tier: T1
    #[serde(default)]
    pub business_purpose: ExpenseReportGeneralInformationBusinessPurpose,
    ///  Conference / event name. Phase 5: synthesized from canonical_event_name across
    /// conference receipts + supporting docs. T1 fallback when no conference docs uploaded.
    /// Source tier: T4
    /// Infer from:  Synthesis: canonical_event_name from synthesis_conference_bundle
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub event_name: Wrapped<String>,
    ///  At least one reason must be selected. Determines required approvals.
    /// Depends on:  general_information.payee.affiliation
    /// Source tier: T3
    #[serde(default)]
    pub student_certification: ExpenseReportGeneralInformationStudentCertification,
    ///  Faculty member or approver name
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_by: Option<String>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionSummary {
    ///  Format: ERxxxxxxx. Assigned by the system or existing system.
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub transaction_number: Wrapped<String>,
    ///  Date of the expense (earliest expense date across all receipts in the bundle).
    /// Source tier: T3
    /// Infer from:  Earliest common.date across transaction_lines
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub transaction_date: Wrapped<IsoDate>,
    ///  FA-entered submission state.
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ExpenseReportTransactionSummaryStatusEnum>,
    ///  Sum of all transaction lines, converted to USD
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub total_usd: Wrapped<f64>,

}

///  Reference to the uploaded document this line was extracted from. Filled by the extractor
/// from the same document that produced the line's other fields.
/// Source tier: T3
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemCommonSourceDocument {
    ///  Filename the FA uploaded — system context, not extracted.
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    ///  Document kind chosen by the FA at upload time.
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_type: Option<ExpenseReportTransactionLinesItemCommonSourceDocumentDocumentTypeEnum>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemCommon {
    /// Source tier: T3
    /// Infer from:  Date on the receipt
    /// Validation rule:  Must fall within trip date window
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub date: Wrapped<IsoDate>,
    ///  Amount in USD. If original currency is foreign, this is the converted amount.
    /// Source tier: T3
    /// Infer from:  Receipt amount × exchange rate (if foreign)
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub line_amount_usd: Wrapped<f64>,
    /// Conditionally required when:  expense_type in [airfare_foreign,
    /// Conditionally required when: gift_card_employee_foreign, gifts_foreign_activity,
    /// Conditionally required when: ground_transportation_foreign, lodging_foreign]
    /// Source tier: T3
    /// Infer from:  Currency symbol/code on receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub original_currency: Wrapped<String>,
    /// Conditionally required when:  expense_type in [airfare_foreign,
    /// Conditionally required when: gift_card_employee_foreign, gifts_foreign_activity,
    /// Conditionally required when: ground_transportation_foreign, lodging_foreign]
    /// Source tier: T3
    /// Infer from:  Amount as printed on receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub original_amount: Wrapped<f64>,
    /// Conditionally required when:  expense_type in [airfare_foreign,
    /// Conditionally required when: gift_card_employee_foreign, gifts_foreign_activity,
    /// Conditionally required when: ground_transportation_foreign, lodging_foreign]
    /// Source tier: T2
    /// Infer from:  Historical rate for common.date from exchange rate API
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub exchange_rate: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  LLM classifies from receipt content
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub expense_type: Wrapped<ExpenseReportTransactionLinesItemCommonExpenseTypeEnum>,
    ///  Reiterate dates, foreign currency details, any context
    /// Source tier: T3
    /// Infer from:  Generated from receipt details and trip context
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub remarks: Wrapped<String>,
    /// Conditionally required when:  expense_type in [airfare_foreign,
    /// Conditionally required when: gift_card_employee_foreign, gifts_foreign_activity,
    /// Conditionally required when: ground_transportation_foreign, lodging_foreign]
    /// Source tier: T3
    /// Infer from:  Destination country from flight/hotel docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub country_of_activity: Wrapped<String>,
    /// Conditionally required when:  expense_type in [airfare_foreign,
    /// Conditionally required when: gift_card_employee_foreign, gifts_foreign_activity,
    /// Conditionally required when: ground_transportation_foreign, lodging_foreign]
    /// Source tier: T3
    /// Infer from:  Conference registration → 'conference'; otherwise from context
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub foreign_activity_type: Wrapped<ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum>,
    ///  Reference to the uploaded document this line was extracted from. Filled by the
    /// extractor from the same document that produced the line's other fields.
    /// Source tier: T3
    #[serde(default)]
    pub source_document: ExpenseReportTransactionLinesItemCommonSourceDocument,

}

/// Conditionally required when:  expense_type in [airfare_domestic, airfare_foreign]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemAirfareDetails {
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub travelers_name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  E-ticket receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub ticket_number: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub ticket_amount: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation format/header; default 'other' if unrecognized
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub booking_method: Wrapped<ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation or ticket
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub airline: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub class_of_ticket: Wrapped<ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum>,
    ///  IATA code
    /// Source tier: T3
    /// Infer from:  Itinerary
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub departure_airport: Wrapped<String>,
    ///  IATA code
    /// Source tier: T3
    /// Infer from:  Itinerary
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub destination_airport: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Itinerary shows return leg
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub round_trip: Wrapped<bool>,

}

/// Conditionally required when:  expense_type in [lodging_domestic, lodging_foreign]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemLodgingDetails {
    /// Source tier: T3
    /// Infer from:  Hotel folio header
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub hotel_name: Wrapped<String>,
    ///  City, Country
    /// Source tier: T3
    /// Infer from:  Hotel folio address
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Hotel folio
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub check_in_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Hotel folio
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub check_out_date: Wrapped<IsoDate>,
    /// Source tier: T2
    /// Infer from:  Computed: check_out_date - check_in_date
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub number_of_nights: Wrapped<f64>,
    ///  Single per-night rate the FA sees in the workbench. Computed by reduction as the mean
    /// of extras.nightly_rates[].rate from the per-document JSON — handles flat-rate folios
    /// trivially (mean of N identical values = the value) and folios with varying nightly rates
    /// (conference-rate nights + walk-in nights) by averaging.
    /// Source tier: T2
    /// Infer from:  reduce.daily_rate: mean(extras.nightly_rates[].rate)
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub daily_rate: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  If hotel matches conference venue → conference_hotel; else check booking
    /// Infer from: source
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub booking_method: Wrapped<ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum>,
    /// Source tier: T3
    /// Infer from:  Context from payee; default false
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub is_shared_lodging: Wrapped<bool>,
    ///  ERxxxxxxx of the other traveler's report
    /// Conditionally required when:  lodging_details.is_shared_lodging == true
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_with_transaction_number: Option<String>,
    ///  Number of nights flagged as personal (outside conference dates)
    /// Source tier: T2
    /// Infer from:  Compare hotel dates against conference dates from registration
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub personal_nights_excluded: Wrapped<f64>,

}

/// Conditionally required when:  expense_type in [ground_transportation_foreign,
/// Conditionally required when: ground_transportation_domestic]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemGroundTransportDetails {
    /// Source tier: T3
    /// Infer from:  Uber/Lyft receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub origin: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Uber/Lyft receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub destination: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Receipt header (Uber, Lyft, taxi company, etc.)
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub service_provider: Wrapped<String>,
    ///  If true, missing receipt form is used instead
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_receipt: Option<bool>,

}

/// Source tier: T1
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<IsoDate>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakfast: Option<bool>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lunch: Option<bool>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dinner: Option<bool>,

}

///  Which meals the conference provides, by day. Feeds into per diem deductions. Phase 5 v1: FA
/// fills this manually; could become T2 from supporting_conference_doc extraction later.
/// Source tier: T1
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Vec<ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem>>,

}

/// Conditionally required when:  expense_type == conference_registration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetails {
    /// Source tier: T3
    /// Infer from:  Registration receipt header / ticket text
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub conference_name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Registration receipt — order confirmation / registration ID
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub order_number: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Registration receipt — what was purchased (e.g. 'Main Conference Only',
    /// Infer from: 'Workshops/Tutorials', 'Full Pass + Banquet')
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub ticket_type: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Registration receipt — attendee/registrant field
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub attendee_name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Registration receipt format/branding (Whova logo, ACM portal style, etc.);
    /// Infer from: default 'other' if unrecognized
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub registration_system: Wrapped<ExpenseReportTransactionLinesItemConferenceRegistrationDetailsRegistrationSystemEnum>,
    /// Source tier: T2
    /// Infer from:  Reduction: min of supporting_conference_doc.scheduled_dates; T1 fallback
    /// Infer from: when no supporting docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub conference_start_date: Wrapped<IsoDate>,
    /// Source tier: T2
    /// Infer from:  Reduction: max of supporting_conference_doc.scheduled_dates; T1 fallback
    /// Infer from: when no supporting docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub conference_end_date: Wrapped<IsoDate>,
    ///  Which meals the conference provides, by day. Feeds into per diem deductions. Phase 5
    /// v1: FA fills this manually; could become T2 from supporting_conference_doc extraction
    /// later.
    /// Source tier: T1
    #[serde(default)]
    pub meals_included: ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded,

}

/// Source tier: T1
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemMealDetailsAttendeesItem {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affiliation: Option<String>,

}

/// Conditionally required when:  expense_type in [business_meal, group_travel_meal]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemMealDetails {
    /// Source tier: T3
    /// Infer from:  Receipt header
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub venue_name: Wrapped<String>,
    ///  Only the payee knows who attended
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attendees: Vec<ExpenseReportTransactionLinesItemMealDetailsAttendeesItem>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meal_purpose: Option<String>,
    /// Conditionally required when:  meal_details.has_alcohol_on_receipt == true
    /// Source tier: T3
    /// Infer from:  Itemized receipt — sum of alcohol line items
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub alcohol_amount: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  Receipt
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub tip_amount: Wrapped<f64>,
    ///  Flag for validation: if true but expense_type is non-alcohol variant, raise
    /// irregularity
    /// Source tier: T3
    /// Infer from:  Scan itemized receipt for alcohol items
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub has_alcohol_on_receipt: Wrapped<bool>,

}

/// Conditionally required when:  expense_type == car_rental
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemCarRentalDetails {
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub rental_company: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub pickup_location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub return_location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub rental_start_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub rental_end_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub vehicle_class: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental receipt line items
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub insurance_included: Wrapped<bool>,

}

/// Conditionally required when:  expense_type in [gift_card_employee_foreign,
/// Conditionally required when: gifts_foreign_activity]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemGiftDetails {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_name: Option<String>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipient_relationship: Option<String>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gift_purpose: Option<String>,

}

/// Conditionally required when:  expense_type == human_subject_incentive
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItemHumanSubjectDetails {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irb_protocol_number: Option<String>,
    /// Source tier: T3
    /// Infer from:  Distribution log
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub number_of_subjects: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  Distribution log
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub per_subject_amount: Wrapped<f64>,

}

///  One entry per distinct expense
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportTransactionLinesItem {
    #[serde(default)]
    pub common: ExpenseReportTransactionLinesItemCommon,
    /// Conditionally required when:  expense_type in [airfare_domestic, airfare_foreign]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airfare_details: Option<ExpenseReportTransactionLinesItemAirfareDetails>,
    /// Conditionally required when:  expense_type in [lodging_domestic, lodging_foreign]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lodging_details: Option<ExpenseReportTransactionLinesItemLodgingDetails>,
    /// Conditionally required when:  expense_type in [ground_transportation_foreign,
    /// Conditionally required when: ground_transportation_domestic]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_transport_details: Option<ExpenseReportTransactionLinesItemGroundTransportDetails>,
    /// Conditionally required when:  expense_type == conference_registration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conference_registration_details: Option<ExpenseReportTransactionLinesItemConferenceRegistrationDetails>,
    /// Conditionally required when:  expense_type in [business_meal, group_travel_meal]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meal_details: Option<ExpenseReportTransactionLinesItemMealDetails>,
    /// Conditionally required when:  expense_type == car_rental
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub car_rental_details: Option<ExpenseReportTransactionLinesItemCarRentalDetails>,
    /// Conditionally required when:  expense_type in [gift_card_employee_foreign,
    /// Conditionally required when: gifts_foreign_activity]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gift_details: Option<ExpenseReportTransactionLinesItemGiftDetails>,
    /// Conditionally required when:  expense_type == human_subject_incentive
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_subject_details: Option<ExpenseReportTransactionLinesItemHumanSubjectDetails>,

}

/// Source tier: T2
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportPerDiemExpensesItemMealDeductionsItem {
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub date: Wrapped<IsoDate>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub breakfast_provided: Wrapped<bool>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub lunch_provided: Wrapped<bool>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub dinner_provided: Wrapped<bool>,
    ///  Computed from per diem rate breakdown
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub deduction_amount: Wrapped<f64>,

}

/// Source tier: T2
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportPerDiemExpensesItemReimbursementSummaryItem {
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub date: Wrapped<IsoDate>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub per_diem_amount: Wrapped<f64>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub meal_deduction: Wrapped<f64>,
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub net_amount: Wrapped<f64>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportPerDiemExpensesItem {
    /// Source tier: T3
    /// Infer from:  Destination from flight/hotel docs determines domestic vs. international;
    /// Infer from: location determines AK/HI vs. continental
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub expense_type: Wrapped<ExpenseReportPerDiemExpensesItemExpenseTypeEnum>,
    /// Source tier: T3
    /// Infer from:  Trip start from flight itinerary
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub start_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Trip end from flight itinerary
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub end_date: Wrapped<IsoDate>,
    /// Source tier: T2
    /// Infer from:  Computed: end_date - start_date + 1
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub number_of_days: Wrapped<f64>,
    ///  City, State/Country
    /// Source tier: T3
    /// Infer from:  Destination from flight or hotel docs
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Derived from location
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub country_of_activity: Wrapped<String>,
    /// Conditionally required when:  expense_type in [international_lodging,
    /// Conditionally required when: international_meals]
    /// Source tier: T3
    /// Infer from:  Inferred from conference registration or trip context
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub foreign_activity_type: Wrapped<ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum>,
    ///  Daily rate in USD from GSA (domestic) or State Dept (foreign)
    /// Source tier: T2
    /// Infer from:  API lookup by location + date
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub per_diem_rate: Wrapped<f64>,
    /// Source tier: T3
    /// Infer from:  Generated summary of dates and location
    #[serde(default, skip_serializing_if = "Wrapped::is_unknown")]
    pub remarks: Wrapped<String>,
    ///  One entry per day. Pre-filled from conference meal schedule.
    /// Source tier: T2
    /// Infer from:  Cross-reference conference_registration_details.meals_included with trip
    /// Infer from: dates
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meal_deductions: Vec<ExpenseReportPerDiemExpensesItemMealDeductionsItem>,
    ///  One entry per day. Auto-computed.
    /// Source tier: T2
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reimbursement_summary: Vec<ExpenseReportPerDiemExpensesItemReimbursementSummaryItem>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportMileageExpensesItem {
}

/// Source tier: T1
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportAllocationAndApproversBeneficiaryListItem {
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReportAllocationAndApprovers {
    ///  Are there any beneficiaries other than the payee?
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_beneficiaries: Option<bool>,
    /// Conditionally required when:  allocation_and_approvers.other_beneficiaries == true
    /// Source tier: T1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beneficiary_list: Option<Vec<ExpenseReportAllocationAndApproversBeneficiaryListItem>>,

}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExpenseReport {
    #[serde(default)]
    pub general_information: ExpenseReportGeneralInformation,
    #[serde(default)]
    pub transaction_summary: ExpenseReportTransactionSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_lines: Option<Vec<ExpenseReportTransactionLinesItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_diem_expenses: Option<Vec<ExpenseReportPerDiemExpensesItem>>,
    ///  Placeholder — details to be filled after FA consultation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mileage_expenses: Option<Vec<ExpenseReportMileageExpensesItem>>,
    #[serde(default)]
    pub allocation_and_approvers: ExpenseReportAllocationAndApprovers,

}

pub type ExpenseReportModel = ExpenseReport;
