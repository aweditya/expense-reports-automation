"""Auto-generated typed model from schema.yaml. Do not edit manually."""

from __future__ import annotations

from datetime import date
from decimal import Decimal
from typing import Literal, NotRequired, Required, TypedDict, TypeAlias

SCHEMA_VERSION = "0.1.0"

ExpenseReportGeneralInformationCategoryEnum: TypeAlias = Literal[
    "expenses_domestic",
    "expenses_foreign",
    "athletic_use_only",
    "hr_use_only",
    "human_subjects",
    "relocation",
]

ExpenseReportGeneralInformationPayeeAffiliationEnum: TypeAlias = Literal["stanford_student", "stanford_postdoc", "stanford_faculty", "stanford_staff", "other"]

ExpenseReportGeneralInformationRushProcessingEnum: TypeAlias = Literal["yes", "no"]

ExpenseReportTransactionSummaryTransactionTypeEnum: TypeAlias = Literal["domestic", "foreign"]

ExpenseReportTransactionSummaryStatusEnum: TypeAlias = Literal["draft", "submitted", "approved", "returned", "paid"]

ExpenseReportTransactionLinesItemCommonExpenseTypeEnum: TypeAlias = Literal[
    "adjusted_per_diem",
    "airfare_domestic",
    "airfare_foreign",
    "ancillary_airline_fee",
    "business_meal",
    "business_meal_with_alcohol",
    "car_rental",
    "conference_registration",
    "gift_card_employee_foreign",
    "gifts_foreign_activity",
    "ground_transportation_foreign",
    "ground_transportation_domestic",
    "group_travel_meal",
    "group_travel_meal_with_alcohol",
    "human_subject_incentive",
    "lodging_domestic",
    "lodging_foreign",
]

ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum: TypeAlias = Literal["conference", "research_collaboration", "fieldwork", "other"]

ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum: TypeAlias = Literal[
    "receipt",
    "booking_confirmation",
    "price_comparison",
    "currency_conversion",
    "conference_program",
    "missing_receipt_form",
    "other",
]

ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum: TypeAlias = Literal[
    "stanford_travel_egencia",
    "stanford_travel_key_travel",
    "stanford_travel_connect_ua",
    "stanford_travel_connect_dl",
    "stanford_travel_connect_aa",
    "stanford_travel_connect_as",
    "stanford_travel_connect_ha",
    "other",
]

ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum: TypeAlias = Literal["coach", "premium_economy", "business", "first"]

ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum: TypeAlias = Literal["payee_provided", "system_generated"]

ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum: TypeAlias = Literal["conference_hotel", "stanford_travel_egencia", "stanford_travel_key_travel", "other"]

ExpenseReportPerDiemExpensesItemExpenseTypeEnum: TypeAlias = Literal[
    "alaska_hawaii_lodging",
    "alaska_hawaii_meals",
    "continental_us_lodging",
    "continental_us_meals",
    "international_lodging",
    "international_meals",
]

ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum: TypeAlias = Literal["conference", "research_collaboration", "fieldwork", "other"]

class ExpenseReportGeneralInformationPayee(TypedDict, total=False):
    # Person being reimbursed
    name: Required[str]
    affiliation: Required[ExpenseReportGeneralInformationPayeeAffiliationEnum]

class ExpenseReportGeneralInformationBusinessPurpose(TypedDict, total=False):
    # Structured purpose statement. First 30 chars of the combined text serve as a lookup key.
    who: Required[str]
    what: Required[str]
    when: Required[str]
    where: Required[str]
    why: Required[str]
    # Auto-generated: <name_abbrev><advisor_initials><conference_abbrev>
    key_30char: Required[str]

class ExpenseReportGeneralInformationStudentCertification(TypedDict, total=False):
    # At least one reason must be selected. Determines required approvals.
    # Requires faculty approval
    supports_faculty_research: NotRequired[bool]
    # Requires faculty approval + conference program attachment
    presenting_at_conference: NotRequired[bool]
    # Requires faculty approval. Not applicable to post-docs.
    # Depends on:  general_information.payee.affiliation
    integral_to_degree_work: NotRequired[bool]
    related_to_employment: NotRequired[bool]
    other: NotRequired[bool]
    # Conditionally required when:  student_certification.other == true
    other_explanation: NotRequired[str]

class ExpenseReportGeneralInformation(TypedDict, total=False):
    category: Required[ExpenseReportGeneralInformationCategoryEnum]
    # Person being reimbursed
    payee: Required[ExpenseReportGeneralInformationPayee]
    # Defaults to 'no' unless explicitly requested
    rush_processing: Required[ExpenseReportGeneralInformationRushProcessingEnum]
    # Always 'electronic' — system pre-fills
    payment_method: Required[str]
    # Structured purpose statement. First 30 chars of the combined text serve as a lookup key.
    business_purpose: Required[ExpenseReportGeneralInformationBusinessPurpose]
    # Format: <lab_name> + <Foreign Expenses | Domestic Expenses>
    event_name: Required[str]
    # At least one reason must be selected. Determines required approvals.
    # Depends on:  general_information.payee.affiliation
    student_certification: Required[ExpenseReportGeneralInformationStudentCertification]
    # Faculty member or approver name
    authorized_by: Required[str]

class ExpenseReportTransactionSummary(TypedDict, total=False):
    transaction_type: Required[ExpenseReportTransactionSummaryTransactionTypeEnum]
    # Format: ERxxxxxxx. Assigned by the system or existing system.
    transaction_number: NotRequired[str]
    transaction_date: Required[date]
    status: NotRequired[ExpenseReportTransactionSummaryStatusEnum]
    # Sum of all transaction lines, converted to USD
    total_usd: Required[Decimal]

class ExpenseReportTransactionLinesItemCommonSourceDocumentsItem(TypedDict, total=False):
    filename: NotRequired[str]
    document_type: NotRequired[ExpenseReportTransactionLinesItemCommonSourceDocumentsItemDocumentTypeEnum]

class ExpenseReportTransactionLinesItemCommon(TypedDict, total=False):
    date: Required[date]
    # Amount in USD. If original currency is foreign, this is the converted amount.
    line_amount_usd: Required[Decimal]
    # Conditionally required when:  general_information.category == expenses_foreign
    original_currency: NotRequired[str]
    # Conditionally required when:  general_information.category == expenses_foreign
    original_amount: NotRequired[Decimal]
    # Conditionally required when:  general_information.category == expenses_foreign
    exchange_rate: NotRequired[Decimal]
    expense_type: Required[ExpenseReportTransactionLinesItemCommonExpenseTypeEnum]
    # Reiterate dates, foreign currency details, any context
    remarks: Required[str]
    # Conditionally required when:  general_information.category == expenses_foreign
    country_of_activity: NotRequired[str]
    # Conditionally required when:  general_information.category == expenses_foreign
    foreign_activity_type: NotRequired[ExpenseReportTransactionLinesItemCommonForeignActivityTypeEnum]
    # References to uploaded documents supporting this line
    source_documents: Required[list[ExpenseReportTransactionLinesItemCommonSourceDocumentsItem]]

class ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonComparableFaresItem(TypedDict, total=False):
    airline: NotRequired[str]
    amount: NotRequired[Decimal]
    class_: NotRequired[str]

class ExpenseReportTransactionLinesItemAirfareDetailsPriceComparison(TypedDict, total=False):
    # System-generated if not provided by payee
    # Date the comparison was generated
    comparison_date: NotRequired[date]
    comparable_fares: NotRequired[list[ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonComparableFaresItem]]
    source: NotRequired[ExpenseReportTransactionLinesItemAirfareDetailsPriceComparisonSourceEnum]

class ExpenseReportTransactionLinesItemAirfareDetails(TypedDict, total=False):
    travelers_name: Required[str]
    ticket_number: Required[str]
    ticket_amount: Required[Decimal]
    booking_method: Required[ExpenseReportTransactionLinesItemAirfareDetailsBookingMethodEnum]
    airline: Required[str]
    class_of_ticket: Required[ExpenseReportTransactionLinesItemAirfareDetailsClassOfTicketEnum]
    # IATA code
    departure_airport: Required[str]
    # IATA code
    destination_airport: Required[str]
    round_trip: Required[bool]
    # System-generated if not provided by payee
    price_comparison: Required[ExpenseReportTransactionLinesItemAirfareDetailsPriceComparison]

class ExpenseReportTransactionLinesItemLodgingDetails(TypedDict, total=False):
    hotel_name: Required[str]
    # City, Country
    location: Required[str]
    check_in_date: Required[date]
    check_out_date: Required[date]
    number_of_nights: Required[Decimal]
    # In original currency
    daily_rate: Required[Decimal]
    booking_method: Required[ExpenseReportTransactionLinesItemLodgingDetailsBookingMethodEnum]
    is_shared_lodging: Required[bool]
    # ERxxxxxxx of the other traveler's report
    # Conditionally required when:  lodging_details.is_shared_lodging == true
    shared_with_transaction_number: NotRequired[str]
    # Number of nights flagged as personal (outside conference dates)
    personal_nights_excluded: NotRequired[Decimal]

class ExpenseReportTransactionLinesItemGroundTransportDetails(TypedDict, total=False):
    origin: Required[str]
    destination: Required[str]
    service_provider: Required[str]
    # If true, missing receipt form is used instead
    missing_receipt: Required[bool]

class ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem(TypedDict, total=False):
    date: NotRequired[date]
    breakfast: NotRequired[bool]
    lunch: NotRequired[bool]
    dinner: NotRequired[bool]

class ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded(TypedDict, total=False):
    # Which meals the conference provides, by day. Feeds into per diem deductions.
    schedule: NotRequired[list[ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncludedScheduleItem]]

class ExpenseReportTransactionLinesItemConferenceRegistrationDetails(TypedDict, total=False):
    conference_name: Required[str]
    order_number: Required[str]
    conference_start_date: Required[date]
    conference_end_date: Required[date]
    # Which meals the conference provides, by day. Feeds into per diem deductions.
    meals_included: Required[ExpenseReportTransactionLinesItemConferenceRegistrationDetailsMealsIncluded]

class ExpenseReportTransactionLinesItemMealDetailsAttendeesItem(TypedDict, total=False):
    name: NotRequired[str]
    affiliation: NotRequired[str]

class ExpenseReportTransactionLinesItemMealDetails(TypedDict, total=False):
    venue_name: Required[str]
    # Only the payee knows who attended
    attendees: Required[list[ExpenseReportTransactionLinesItemMealDetailsAttendeesItem]]
    meal_purpose: Required[str]
    # Conditionally required when:  expense_type in [business_meal_with_alcohol,
    # Conditionally required when: group_travel_meal_with_alcohol]
    alcohol_amount: NotRequired[Decimal]
    tip_amount: NotRequired[Decimal]
    # Flag for validation: if true but expense_type is non-alcohol variant, raise irregularity
    has_alcohol_on_receipt: Required[bool]

class ExpenseReportTransactionLinesItemCarRentalDetails(TypedDict, total=False):
    rental_company: Required[str]
    pickup_location: Required[str]
    return_location: Required[str]
    rental_start_date: Required[date]
    rental_end_date: Required[date]
    vehicle_class: Required[str]
    insurance_included: Required[bool]

class ExpenseReportTransactionLinesItemGiftDetails(TypedDict, total=False):
    recipient_name: Required[str]
    recipient_relationship: Required[str]
    gift_purpose: Required[str]

class ExpenseReportTransactionLinesItemHumanSubjectDetails(TypedDict, total=False):
    irb_protocol_number: Required[str]
    number_of_subjects: Required[Decimal]
    per_subject_amount: Required[Decimal]

class ExpenseReportTransactionLinesItem(TypedDict, total=False):
    # One entry per distinct expense
    common: Required[ExpenseReportTransactionLinesItemCommon]
    # Conditionally required when:  expense_type in [airfare_domestic, airfare_foreign]
    airfare_details: NotRequired[ExpenseReportTransactionLinesItemAirfareDetails]
    # Conditionally required when:  expense_type in [lodging_domestic, lodging_foreign]
    lodging_details: NotRequired[ExpenseReportTransactionLinesItemLodgingDetails]
    # Conditionally required when:  expense_type in [ground_transportation_foreign,
    # Conditionally required when: ground_transportation_domestic]
    ground_transport_details: NotRequired[ExpenseReportTransactionLinesItemGroundTransportDetails]
    # Conditionally required when:  expense_type == conference_registration
    conference_registration_details: NotRequired[ExpenseReportTransactionLinesItemConferenceRegistrationDetails]
    # Conditionally required when:  expense_type in [business_meal, business_meal_with_alcohol,
    # Conditionally required when: group_travel_meal, group_travel_meal_with_alcohol]
    meal_details: NotRequired[ExpenseReportTransactionLinesItemMealDetails]
    # Conditionally required when:  expense_type == car_rental
    car_rental_details: NotRequired[ExpenseReportTransactionLinesItemCarRentalDetails]
    # Conditionally required when:  expense_type in [gift_card_employee_foreign,
    # Conditionally required when: gifts_foreign_activity]
    gift_details: NotRequired[ExpenseReportTransactionLinesItemGiftDetails]
    # Conditionally required when:  expense_type == human_subject_incentive
    human_subject_details: NotRequired[ExpenseReportTransactionLinesItemHumanSubjectDetails]

class ExpenseReportPerDiemExpensesItemMealDeductionsItem(TypedDict, total=False):
    date: NotRequired[date]
    breakfast_provided: NotRequired[bool]
    lunch_provided: NotRequired[bool]
    dinner_provided: NotRequired[bool]
    # Computed from per diem rate breakdown
    deduction_amount: NotRequired[Decimal]

class ExpenseReportPerDiemExpensesItemReimbursementSummaryItem(TypedDict, total=False):
    date: NotRequired[date]
    per_diem_amount: NotRequired[Decimal]
    meal_deduction: NotRequired[Decimal]
    net_amount: NotRequired[Decimal]

class ExpenseReportPerDiemExpensesItem(TypedDict, total=False):
    expense_type: Required[ExpenseReportPerDiemExpensesItemExpenseTypeEnum]
    start_date: Required[date]
    end_date: Required[date]
    number_of_days: Required[Decimal]
    # City, State/Country
    location: Required[str]
    country_of_activity: Required[str]
    # Conditionally required when:  expense_type in [international_lodging, international_meals]
    foreign_activity_type: NotRequired[ExpenseReportPerDiemExpensesItemForeignActivityTypeEnum]
    # Daily rate in USD from GSA (domestic) or State Dept (foreign)
    per_diem_rate: Required[Decimal]
    remarks: Required[str]
    # One entry per day. Pre-filled from conference meal schedule.
    meal_deductions: Required[list[ExpenseReportPerDiemExpensesItemMealDeductionsItem]]
    # One entry per day. Auto-computed.
    reimbursement_summary: Required[list[ExpenseReportPerDiemExpensesItemReimbursementSummaryItem]]

class ExpenseReportMileageExpensesItem(TypedDict, total=False):
    pass

class ExpenseReportAllocationAndApproversBeneficiaryListItem(TypedDict, total=False):
    name: NotRequired[str]
    relationship: NotRequired[str]

class ExpenseReportAllocationAndApprovers(TypedDict, total=False):
    # Are there any beneficiaries other than the payee?
    other_beneficiaries: Required[bool]
    # Conditionally required when:  allocation_and_approvers.other_beneficiaries == true
    beneficiary_list: NotRequired[list[ExpenseReportAllocationAndApproversBeneficiaryListItem]]

class ExpenseReport(TypedDict, total=False):
    general_information: Required[ExpenseReportGeneralInformation]
    transaction_summary: Required[ExpenseReportTransactionSummary]
    transaction_lines: NotRequired[list[ExpenseReportTransactionLinesItem]]
    per_diem_expenses: NotRequired[list[ExpenseReportPerDiemExpensesItem]]
    # Placeholder — details to be filled after FA consultation
    mileage_expenses: NotRequired[list[ExpenseReportMileageExpensesItem]]
    allocation_and_approvers: Required[ExpenseReportAllocationAndApprovers]

ExpenseReportModel = ExpenseReport
