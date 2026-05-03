// Auto-generated typed model from schema.yaml. Do not edit manually.

use crate::meta::Wrapped;

pub const SCHEMA_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DecimalAmount(pub String);

impl From<String> for DecimalAmount {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for DecimalAmount {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Source tier: T3
/// Infer from:  Destination in flight/hotel docs. Foreign destination → expenses_foreign;
/// Infer from: domestic → expenses_domestic. Other categories require explicit context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportGeneralInformationCategoryEnum {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportGeneralInformationPayeeAffiliationEnum {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportGeneralInformationRushProcessingEnum {
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

///  FA-entered. Typically matches general_information.category but the portal stores it as a
/// separate field.
/// Source tier: T1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionSummaryTransactionTypeEnum {
    Domestic,
    Foreign,
}

impl ExpenseReportTransactionSummaryTransactionTypeEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Domestic => "domestic",
            Self::Foreign => "foreign",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionSummaryTransactionTypeEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "domestic" => Ok(Self::Domestic),
            "foreign" => Ok(Self::Foreign),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionSummaryTransactionTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

///  FA-entered submission state.
/// Source tier: T1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionSummaryStatusEnum {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemCommonExpenseTypeEnum {
    AdjustedPerDiem,
    AirfareDomestic,
    AirfareForeign,
    AncillaryAirlineFee,
    BusinessMeal,
    BusinessMealWithAlcohol,
    CarRental,
    ConferenceRegistration,
    GiftCardEmployeeForeign,
    GiftsForeignActivity,
    GroundTransportationForeign,
    GroundTransportationDomestic,
    GroupTravelMeal,
    GroupTravelMealWithAlcohol,
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
            Self::BusinessMealWithAlcohol => "business_meal_with_alcohol",
            Self::CarRental => "car_rental",
            Self::ConferenceRegistration => "conference_registration",
            Self::GiftCardEmployeeForeign => "gift_card_employee_foreign",
            Self::GiftsForeignActivity => "gifts_foreign_activity",
            Self::GroundTransportationForeign => "ground_transportation_foreign",
            Self::GroundTransportationDomestic => "ground_transportation_domestic",
            Self::GroupTravelMeal => "group_travel_meal",
            Self::GroupTravelMealWithAlcohol => "group_travel_meal_with_alcohol",
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
            "business_meal_with_alcohol" => Ok(Self::BusinessMealWithAlcohol),
            "car_rental" => Ok(Self::CarRental),
            "conference_registration" => Ok(Self::ConferenceRegistration),
            "gift_card_employee_foreign" => Ok(Self::GiftCardEmployeeForeign),
            "gifts_foreign_activity" => Ok(Self::GiftsForeignActivity),
            "ground_transportation_foreign" => Ok(Self::GroundTransportationForeign),
            "ground_transportation_domestic" => Ok(Self::GroundTransportationDomestic),
            "group_travel_meal" => Ok(Self::GroupTravelMeal),
            "group_travel_meal_with_alcohol" => Ok(Self::GroupTravelMealWithAlcohol),
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

/// Conditionally required when:  general_information.category == expenses_foreign
/// Source tier: T3
/// Infer from:  Conference registration → 'conference'; otherwise from context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum {
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

/// Source tier: T3
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum {
    Receipt,
    BookingConfirmation,
    PriceComparison,
    CurrencyConversion,
    ConferenceProgram,
    MissingReceiptForm,
    Other,
}

impl ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum {
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

impl core::str::FromStr for ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum {
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

impl core::fmt::Display for ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  Booking confirmation format/header; default 'other' if unrecognized
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum {
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

/// Source tier: T2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum {
    PayeeProvided,
    SystemGenerated,
}

impl ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PayeeProvided => "payee_provided",
            Self::SystemGenerated => "system_generated",
        }
    }
}

impl core::str::FromStr for ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "payee_provided" => Ok(Self::PayeeProvided),
            "system_generated" => Ok(Self::SystemGenerated),
            _ => Err("invalid enum value"),
        }
    }
}

impl core::fmt::Display for ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Source tier: T3
/// Infer from:  If hotel matches conference venue → conference_hotel; else check booking source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum {
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
/// Infer from:  Destination from flight/hotel docs determines domestic vs. international;
/// Infer from: location determines AK/HI vs. continental
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportPerDiemExpensesItemExpenseTypeEnum {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum {
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
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportGeneralInformationPayee {
    /// Source tier: T3
    /// Infer from:  Traveler name on flight booking or hotel folio
    pub name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Context from uploaded docs or FA input
    pub affiliation: Wrapped<ExpenseReportGeneralInformationPayeeAffiliationEnum>,

}

///  Structured purpose statement entered by the FA. First 30 chars of the combined text serve
/// as a lookup key.
/// Source tier: T1
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportGeneralInformationBusinessPurpose {
    /// Source tier: T1
    /// Infer from:  Suggestion only — payee name and affiliation
    pub who: Wrapped<String>,
    /// Source tier: T1
    /// Infer from:  Suggestion only — conference name from registration receipt; or meeting
    /// Infer from: purpose from context
    pub what: Wrapped<String>,
    /// Source tier: T1
    /// Infer from:  Suggestion only — trip dates from flight itinerary
    pub when: Wrapped<String>,
    /// Source tier: T1
    /// Infer from:  Suggestion only — destination city/country from flight or hotel docs
    pub r#where: Wrapped<String>,
    /// Source tier: T1
    /// Infer from:  Suggestion only — presenting at conference (if name appears in program),
    /// Infer from: research collaboration, etc.
    pub why: Wrapped<String>,
    ///  30-character lookup key entered by the FA
    /// Source tier: T1
    pub key_30char: Wrapped<String>,

}

///  At least one reason must be selected. Determines required approvals.
/// Depends on:  general_information.payee.affiliation
/// Source tier: T3
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportGeneralInformationStudentCertification {
    ///  Requires faculty approval
    /// Source tier: T3
    pub supports_faculty_research: Wrapped<bool>,
    ///  Requires faculty approval + conference program attachment
    /// Source tier: T3
    /// Infer from:  Payee name appears in conference program/agenda
    pub presenting_at_conference: Wrapped<bool>,
    ///  Requires faculty approval. Not applicable to post-docs.
    /// Depends on:  general_information.payee.affiliation
    /// Source tier: T3
    pub integral_to_degree_work: Wrapped<bool>,
    /// Source tier: T3
    pub related_to_employment: Wrapped<bool>,
    /// Source tier: T3
    pub other: Wrapped<bool>,
    /// Conditionally required when:  student_certification.other == true
    /// Source tier: T3
    pub other_explanation: Wrapped<String>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportGeneralInformation {
    /// Source tier: T3
    /// Infer from:  Destination in flight/hotel docs. Foreign destination → expenses_foreign;
    /// Infer from: domestic → expenses_domestic. Other categories require explicit context.
    pub category: Wrapped<ExpenseReportGeneralInformationCategoryEnum>,
    ///  Person being reimbursed
    /// Source tier: T3
    pub payee: ExpenseReportGeneralInformationPayee,
    ///  Defaults to 'no' unless explicitly requested
    /// Source tier: T1
    pub rush_processing: Wrapped<ExpenseReportGeneralInformationRushProcessingEnum>,
    ///  Always 'electronic' — system pre-fills
    /// Source tier: T2
    pub payment_method: Wrapped<String>,
    ///  Structured purpose statement entered by the FA. First 30 chars of the combined text
    /// serve as a lookup key.
    /// Source tier: T1
    pub business_purpose: ExpenseReportGeneralInformationBusinessPurpose,
    ///  Format: <lab_name> + <Foreign Expenses | Domestic Expenses>
    /// Source tier: T3
    /// Infer from:  Lab affiliation + category
    pub event_name: Wrapped<String>,
    ///  At least one reason must be selected. Determines required approvals.
    /// Depends on:  general_information.payee.affiliation
    /// Source tier: T3
    pub student_certification: ExpenseReportGeneralInformationStudentCertification,
    ///  Faculty member or approver name
    /// Source tier: T1
    pub authorized_by: Wrapped<String>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionSummary {
    ///  FA-entered. Typically matches general_information.category but the portal stores it as
    /// a separate field.
    /// Source tier: T1
    pub transaction_type: Wrapped<ExpenseReportTransactionSummaryTransactionTypeEnum>,
    ///  Format: ERxxxxxxx. Assigned by the system or existing system.
    /// Source tier: T2
    pub transaction_number: Wrapped<String>,
    ///  Date of the expense (earliest expense date across all receipts in the bundle).
    /// Source tier: T3
    /// Infer from:  Earliest common.date across transaction_lines
    pub transaction_date: Wrapped<IsoDate>,
    ///  FA-entered submission state.
    /// Source tier: T1
    pub status: Wrapped<ExpenseReportTransactionSummaryStatusEnum>,
    ///  Sum of all transaction lines, converted to USD
    /// Source tier: T2
    pub total_usd: Wrapped<DecimalAmount>,

}

/// Source tier: T3
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemCommonSourceDocumentsItem {
    /// Source tier: T3
    pub filename: Wrapped<String>,
    /// Source tier: T3
    pub document_type: Wrapped<ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemCommon {
    /// Source tier: T3
    /// Infer from:  Date on the receipt
    /// Validation rule:  Must fall within trip date window
    pub date: Wrapped<IsoDate>,
    ///  Amount in USD. If original currency is foreign, this is the converted amount.
    /// Source tier: T3
    /// Infer from:  Receipt amount × exchange rate (if foreign)
    pub line_amount_usd: Wrapped<DecimalAmount>,
    /// Conditionally required when:  general_information.category == expenses_foreign
    /// Source tier: T3
    /// Infer from:  Currency symbol/code on receipt
    pub original_currency: Wrapped<String>,
    /// Conditionally required when:  general_information.category == expenses_foreign
    /// Source tier: T3
    /// Infer from:  Amount as printed on receipt
    pub original_amount: Wrapped<DecimalAmount>,
    /// Conditionally required when:  general_information.category == expenses_foreign
    /// Source tier: T2
    /// Infer from:  Historical rate for common.date from exchange rate API
    pub exchange_rate: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  LLM classifies from receipt content
    pub expense_type: Wrapped<ExpenseReportTransactionLinesItemCommonExpenseTypeEnum>,
    ///  Reiterate dates, foreign currency details, any context
    /// Source tier: T3
    /// Infer from:  Generated from receipt details and trip context
    pub remarks: Wrapped<String>,
    /// Conditionally required when:  general_information.category == expenses_foreign
    /// Source tier: T3
    /// Infer from:  Destination country from flight/hotel docs
    pub country_of_activity: Wrapped<String>,
    /// Conditionally required when:  general_information.category == expenses_foreign
    /// Source tier: T3
    /// Infer from:  Conference registration → 'conference'; otherwise from context
    pub foreign_activity_type: Wrapped<ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum>,
    ///  References to uploaded documents supporting this line. Filled by the extractor from the
    /// same documents that produced the line's other fields.
    /// Source tier: T3
    pub source_documents: Vec<ExpenseReportTransactionLinesItemCommonSourceDocumentsItem>,

}

/// Source tier: T2
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonComparableFaresItem {
    /// Source tier: T2
    pub airline: Wrapped<String>,
    /// Source tier: T2
    pub amount: Wrapped<DecimalAmount>,
    /// Source tier: T2
    pub class: Wrapped<String>,

}

///  System-generated if not provided by payee
/// Source tier: T2
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemAirfareDetailsPriceComparison {
    ///  Date the comparison was generated
    /// Source tier: T2
    pub comparison_date: Wrapped<IsoDate>,
    /// Source tier: T2
    pub comparable_fares: Option<Vec<ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonComparableFaresItem>>,
    /// Source tier: T2
    pub source: Wrapped<ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum>,

}

/// Conditionally required when:  expense_type in [airfare_domestic, airfare_foreign]
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemAirfareDetails {
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    pub travelers_name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  E-ticket receipt
    pub ticket_number: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    pub ticket_amount: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation format/header; default 'other' if unrecognized
    pub booking_method: Wrapped<ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation or ticket
    pub airline: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Booking confirmation
    pub class_of_ticket: Wrapped<ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum>,
    ///  IATA code
    /// Source tier: T3
    /// Infer from:  Itinerary
    pub departure_airport: Wrapped<String>,
    ///  IATA code
    /// Source tier: T3
    /// Infer from:  Itinerary
    pub destination_airport: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Itinerary shows return leg
    pub round_trip: Wrapped<bool>,
    ///  System-generated if not provided by payee
    /// Source tier: T2
    pub price_comparison: ExpenseReportTransactionLinesItemAirfareDetailsPriceComparison,

}

/// Conditionally required when:  expense_type in [lodging_domestic, lodging_foreign]
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemLodgingDetails {
    /// Source tier: T3
    /// Infer from:  Hotel folio header
    pub hotel_name: Wrapped<String>,
    ///  City, Country
    /// Source tier: T3
    /// Infer from:  Hotel folio address
    pub location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Hotel folio
    pub check_in_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Hotel folio
    pub check_out_date: Wrapped<IsoDate>,
    /// Source tier: T2
    /// Infer from:  Computed: check_out_date - check_in_date
    pub number_of_nights: Wrapped<DecimalAmount>,
    ///  In original currency
    /// Source tier: T3
    /// Infer from:  Hotel folio
    pub daily_rate: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  If hotel matches conference venue → conference_hotel; else check booking
    /// Infer from: source
    pub booking_method: Wrapped<ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum>,
    /// Source tier: T3
    /// Infer from:  Context from payee; default false
    pub is_shared_lodging: Wrapped<bool>,
    ///  ERxxxxxxx of the other traveler's report
    /// Conditionally required when:  lodging_details.is_shared_lodging == true
    /// Source tier: T1
    pub shared_with_transaction_number: Wrapped<String>,
    ///  Number of nights flagged as personal (outside conference dates)
    /// Source tier: T2
    /// Infer from:  Compare hotel dates against conference dates from registration
    pub personal_nights_excluded: Wrapped<DecimalAmount>,

}

/// Conditionally required when:  expense_type in [ground_transportation_foreign,
/// Conditionally required when: ground_transportation_domestic]
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemGroundTransportDetails {
    /// Source tier: T3
    /// Infer from:  Uber/Lyft receipt
    pub origin: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Uber/Lyft receipt
    pub destination: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Receipt header (Uber, Lyft, taxi company, etc.)
    pub service_provider: Wrapped<String>,
    ///  If true, missing receipt form is used instead
    /// Source tier: T1
    pub missing_receipt: Wrapped<bool>,

}

/// Source tier: T3
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem {
    /// Source tier: T3
    pub date: Wrapped<IsoDate>,
    /// Source tier: T3
    pub breakfast: Wrapped<bool>,
    /// Source tier: T3
    pub lunch: Wrapped<bool>,
    /// Source tier: T3
    pub dinner: Wrapped<bool>,

}

///  Which meals the conference provides, by day. Feeds into per diem deductions.
/// Source tier: T3
/// Infer from:  Conference program/schedule (e.g., 'lunch provided to all attendees')
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded {
    /// Source tier: T3
    pub schedule: Option<Vec<ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem>>,

}

/// Conditionally required when:  expense_type == conference_registration
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemConferenceRegistrationDetails {
    /// Source tier: T3
    /// Infer from:  Registration receipt
    pub conference_name: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Registration receipt
    pub order_number: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Conference program or registration confirmation
    pub conference_start_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Conference program or registration confirmation
    pub conference_end_date: Wrapped<IsoDate>,
    ///  Which meals the conference provides, by day. Feeds into per diem deductions.
    /// Source tier: T3
    /// Infer from:  Conference program/schedule (e.g., 'lunch provided to all attendees')
    pub meals_included: ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded,

}

/// Source tier: T1
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemMealDetailsAttendeesItem {
    /// Source tier: T1
    pub name: Wrapped<String>,
    /// Source tier: T1
    pub affiliation: Wrapped<String>,

}

/// Conditionally required when:  expense_type in [business_meal, business_meal_with_alcohol,
/// Conditionally required when: group_travel_meal, group_travel_meal_with_alcohol]
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemMealDetails {
    /// Source tier: T3
    /// Infer from:  Receipt header
    pub venue_name: Wrapped<String>,
    ///  Only the payee knows who attended
    /// Source tier: T1
    pub attendees: Vec<ExpenseReportTransactionLinesItemMealDetailsAttendeesItem>,
    /// Source tier: T1
    pub meal_purpose: Wrapped<String>,
    /// Conditionally required when:  expense_type in [business_meal_with_alcohol,
    /// Conditionally required when: group_travel_meal_with_alcohol]
    /// Source tier: T3
    /// Infer from:  Itemized receipt — sum of alcohol line items
    pub alcohol_amount: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  Receipt
    pub tip_amount: Wrapped<DecimalAmount>,
    ///  Flag for validation: if true but expense_type is non-alcohol variant, raise
    /// irregularity
    /// Source tier: T3
    /// Infer from:  Scan itemized receipt for alcohol items
    pub has_alcohol_on_receipt: Wrapped<bool>,

}

/// Conditionally required when:  expense_type == car_rental
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemCarRentalDetails {
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub rental_company: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub pickup_location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub return_location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub rental_start_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub rental_end_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Rental agreement
    pub vehicle_class: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Rental receipt line items
    pub insurance_included: Wrapped<bool>,

}

/// Conditionally required when:  expense_type in [gift_card_employee_foreign,
/// Conditionally required when: gifts_foreign_activity]
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemGiftDetails {
    /// Source tier: T1
    pub recipient_name: Wrapped<String>,
    /// Source tier: T1
    pub recipient_relationship: Wrapped<String>,
    /// Source tier: T1
    pub gift_purpose: Wrapped<String>,

}

/// Conditionally required when:  expense_type == human_subject_incentive
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItemHumanSubjectDetails {
    /// Source tier: T1
    pub irb_protocol_number: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Distribution log
    pub number_of_subjects: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  Distribution log
    pub per_subject_amount: Wrapped<DecimalAmount>,

}

///  One entry per distinct expense
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportTransactionLinesItem {
    pub common: ExpenseReportTransactionLinesItemCommon,
    /// Conditionally required when:  expense_type in [airfare_domestic, airfare_foreign]
    pub airfare_details: Option<ExpenseReportTransactionLinesItemAirfareDetails>,
    /// Conditionally required when:  expense_type in [lodging_domestic, lodging_foreign]
    pub lodging_details: Option<ExpenseReportTransactionLinesItemLodgingDetails>,
    /// Conditionally required when:  expense_type in [ground_transportation_foreign,
    /// Conditionally required when: ground_transportation_domestic]
    pub ground_transport_details: Option<ExpenseReportTransactionLinesItemGroundTransportDetails>,
    /// Conditionally required when:  expense_type == conference_registration
    pub conference_registration_details: Option<ExpenseReportTransactionLinesItemConferenceRegistrationDetails>,
    /// Conditionally required when:  expense_type in [business_meal,
    /// Conditionally required when: business_meal_with_alcohol, group_travel_meal,
    /// Conditionally required when: group_travel_meal_with_alcohol]
    pub meal_details: Option<ExpenseReportTransactionLinesItemMealDetails>,
    /// Conditionally required when:  expense_type == car_rental
    pub car_rental_details: Option<ExpenseReportTransactionLinesItemCarRentalDetails>,
    /// Conditionally required when:  expense_type in [gift_card_employee_foreign,
    /// Conditionally required when: gifts_foreign_activity]
    pub gift_details: Option<ExpenseReportTransactionLinesItemGiftDetails>,
    /// Conditionally required when:  expense_type == human_subject_incentive
    pub human_subject_details: Option<ExpenseReportTransactionLinesItemHumanSubjectDetails>,

}

/// Source tier: T2
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportPerDiemExpensesItemMealDeductionsItem {
    /// Source tier: T2
    pub date: Wrapped<IsoDate>,
    /// Source tier: T2
    pub breakfast_provided: Wrapped<bool>,
    /// Source tier: T2
    pub lunch_provided: Wrapped<bool>,
    /// Source tier: T2
    pub dinner_provided: Wrapped<bool>,
    ///  Computed from per diem rate breakdown
    /// Source tier: T2
    pub deduction_amount: Wrapped<DecimalAmount>,

}

/// Source tier: T2
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportPerDiemExpensesItemReimbursementSummaryItem {
    /// Source tier: T2
    pub date: Wrapped<IsoDate>,
    /// Source tier: T2
    pub per_diem_amount: Wrapped<DecimalAmount>,
    /// Source tier: T2
    pub meal_deduction: Wrapped<DecimalAmount>,
    /// Source tier: T2
    pub net_amount: Wrapped<DecimalAmount>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportPerDiemExpensesItem {
    /// Source tier: T3
    /// Infer from:  Destination from flight/hotel docs determines domestic vs. international;
    /// Infer from: location determines AK/HI vs. continental
    pub expense_type: Wrapped<ExpenseReportPerDiemExpensesItemExpenseTypeEnum>,
    /// Source tier: T3
    /// Infer from:  Trip start from flight itinerary
    pub start_date: Wrapped<IsoDate>,
    /// Source tier: T3
    /// Infer from:  Trip end from flight itinerary
    pub end_date: Wrapped<IsoDate>,
    /// Source tier: T2
    /// Infer from:  Computed: end_date - start_date + 1
    pub number_of_days: Wrapped<DecimalAmount>,
    ///  City, State/Country
    /// Source tier: T3
    /// Infer from:  Destination from flight or hotel docs
    pub location: Wrapped<String>,
    /// Source tier: T3
    /// Infer from:  Derived from location
    pub country_of_activity: Wrapped<String>,
    /// Conditionally required when:  expense_type in [international_lodging,
    /// Conditionally required when: international_meals]
    /// Source tier: T3
    /// Infer from:  Inferred from conference registration or trip context
    pub foreign_activity_type: Wrapped<ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum>,
    ///  Daily rate in USD from GSA (domestic) or State Dept (foreign)
    /// Source tier: T2
    /// Infer from:  API lookup by location + date
    pub per_diem_rate: Wrapped<DecimalAmount>,
    /// Source tier: T3
    /// Infer from:  Generated summary of dates and location
    pub remarks: Wrapped<String>,
    ///  One entry per day. Pre-filled from conference meal schedule.
    /// Source tier: T2
    /// Infer from:  Cross-reference conference_registration_details.meals_included with trip
    /// Infer from: dates
    pub meal_deductions: Vec<ExpenseReportPerDiemExpensesItemMealDeductionsItem>,
    ///  One entry per day. Auto-computed.
    /// Source tier: T2
    pub reimbursement_summary: Vec<ExpenseReportPerDiemExpensesItemReimbursementSummaryItem>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportMileageExpensesItem {
}

/// Source tier: T1
#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportAllocationAndApproversBeneficiaryListItem {
    /// Source tier: T1
    pub name: Wrapped<String>,
    /// Source tier: T1
    pub relationship: Wrapped<String>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReportAllocationAndApprovers {
    ///  Are there any beneficiaries other than the payee?
    /// Source tier: T1
    pub other_beneficiaries: Wrapped<bool>,
    /// Conditionally required when:  allocation_and_approvers.other_beneficiaries == true
    /// Source tier: T1
    pub beneficiary_list: Option<Vec<ExpenseReportAllocationAndApproversBeneficiaryListItem>>,

}

#[derive(Debug, Clone, PartialEq)]
pub struct ExpenseReport {
    pub general_information: ExpenseReportGeneralInformation,
    pub transaction_summary: ExpenseReportTransactionSummary,
    pub transaction_lines: Option<Vec<ExpenseReportTransactionLinesItem>>,
    pub per_diem_expenses: Option<Vec<ExpenseReportPerDiemExpensesItem>>,
    ///  Placeholder — details to be filled after FA consultation
    pub mileage_expenses: Option<Vec<ExpenseReportMileageExpensesItem>>,
    pub allocation_and_approvers: ExpenseReportAllocationAndApprovers,

}

pub type ExpenseReportModel = ExpenseReport;
