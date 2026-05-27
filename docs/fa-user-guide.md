# FA user guide

Operating manual for the deployed expense-report tool. Audience:
Stanford CS/EE Faculty Administrators. No engineering background
assumed.

For questions not covered, contact the engineering owner listed in
the deployment guide.

## Purpose

The tool extracts structured fields from uploaded receipt PDFs and
images, validates them against Stanford expense-portal rules, and
produces two downloadable CSVs (one per portal page). The CSVs
pre-fill the same fields entered manually into Stanford's portal.

This is a data-entry aid. It does not submit to Stanford on the
FA's behalf.

## 1. Sign in

Open the deployed URL (request from the engineering contact;
Stanford SSO required).

After SSO, the landing page is the **dashboard**: a list of
expense reports filed under the signed-in SUNet, plus a
**+ New report** button.

- Click the button to start a new report.
- Click a row to open a previously-filed report's workbench.
- Scope is automatic from SSO. The dashboard never shows another
  FA's reports.

A Google "resource not accessible" page indicates the account is
not in the IAP allowlist. Contact the engineering owner.

## 2. Upload form (`/new`)

Two regions: **report-wide fieldset** and **per-receipt file
rows**.

### Required fields

| Field | Constraint |
|---|---|
| Payee SUNet ID | 2–16 chars, alphanumeric + underscore + hyphen |
| Event name | Short trip or event label |
| Business purpose: Who, What, When, Where, Why | Each required |

Other fieldset values (payee full name, affiliation, authorized
by, payment method, foreign activity type) are optional but
recommended.

### File rows

Each row is one receipt: file input + **kind** dropdown.

| Kind | Use for |
|---|---|
| Meal Receipt | Restaurant bill, food order |
| Ground Transport | Uber, Lyft, taxi, train, bus, transit card, parking |
| Lodging Folio | Hotel itemized bill, Airbnb invoice |
| Airfare / Flight Ticket | Flight booking confirmation |
| Miscellaneous | Posters, printing, conference dinner per-person |
| Membership Dues | ACM, IEEE, USENIX renewals |
| Personal Mileage | Google Maps screenshot of driving distance |

Click **+ Add another file** for additional rows. Allowed file
types: PDF, PNG, JPG, HEIC, WebP.

HEIC photos from iPhone are converted automatically.

Submit fails if a required field is empty. The browser highlights
the offending field.

## 3. Progress page

Submission redirects to the progress page. Updates stream live
per phase: extract → reduce → fx → render → done.

- Refresh is safe. The page re-renders and reconnects to the
  event stream.
- Closing the tab does not lose state. The pipeline continues
  server-side; revisit the URL to resume.
- On completion, the page auto-redirects to the workbench.

If one receipt fails extraction, others continue. The failed
receipt appears as a line in the workbench with the error
detail.

Typical wallclock: ~30 seconds per receipt. A 5-receipt batch
takes approximately 3 minutes.

## 4. Workbench

Three regions: hero, issues rail (left), main column.

### Hero

- Title: "Review & File"
- Status pill: line count + count of unresolved validation issues
- Payee + event subtitle
- CSV download buttons (one per Stanford portal page that has
  data)
- Provenance toggle
- Undo button (visible only when edit history is non-empty)
- **+ Add receipts** button (appends to this report)

### Issues rail

Validation rules that fired. Each issue links to the field it
references. Dismiss acknowledges the issue for the current page
load only; refresh clears dismissals.

Examples:

- Tip exceeds 20% of pre-tax + tax (meal, transport)
- Date falls outside the FA-entered trip window
- Required field missing

### Line cards

One per extracted receipt.

Edit:

1. Click the value text.
2. Edit in place.
3. Press Enter to save (Escape cancels).

The edit persists to disk and Firestore on save.

Delete a line: trash icon on the card. Confirms before deleting.
Edit history is cleared on delete because line indices shift.

Undo last edit: hero button. Reverts the most recent edit. History
retains up to 20 edits.

### Provenance and spot-check

The **Show extraction provenance** toggle reveals the source
quote behind every extracted value.

The eye icon on a field card opens the spot-check panel: the
source PDF or image with the extracted region highlighted.

## 5. CSV downloads

Two CSVs, one per Stanford portal page:

| File | Stanford page | Columns |
|---|---|---|
| `lines-domestic.csv` | Domestic Expense Lines | 7 |
| `lines-foreign.csv` | Foreign Expense Lines | 20 |

The hero shows only the downloads with data rows. Domestic-only
trips produce one CSV.

Upload procedure: open the Stanford portal page, click Browse,
select the CSV, click Load. The portal validates; on success,
click Import.

## 6. Add receipts later

To extend an existing report:

1. Open the workbench URL (from the dashboard or a bookmark).
2. Click **+ Add receipts** in the hero.
3. Attach the new files in the modal; pick kinds; click Process.
4. The progress page shows extraction of only the new files. On
   completion, the workbench refreshes with the combined report.

Prior edits to existing lines are preserved. The undo stack
resets because line indices shift.

## 7. Returning later

Workbench URLs are bookmarkable. Reports persist for 90 days
after last edit (Firestore TTL).

The dashboard at `/` lists past reports filed under the signed-in
SUNet.

## 8. Troubleshooting

| Symptom | Action |
|---|---|
| Tool extracted a wrong number | Click the value, edit, press Enter. Edit persists immediately. |
| Tool extracted a wrong currency or expense type | Click and edit. Dropdowns show valid options. |
| A receipt did not produce a line card | Check the progress page or the workbench issues rail for the failure reason. Re-take the photo if extraction failed. Add via §6. |
| Wrong kind for a receipt | Delete the line and re-upload with the correct kind. Kind cannot be changed in place today. |
| Disagree with a validation rule | Edit the value to bypass, or report the rule to engineering. |
| Page loads indefinitely | Refresh. If still stuck, the URL may predate a deploy that wiped state; report the upload_id to engineering. |
| Google "Forbidden" page | IAP grant expired or SSO session timed out. Re-sign-in. |

## Glossary

| Term | Definition |
|---|---|
| Workbench | The HTML page reviewed after extraction. |
| Line card | One transaction line on the workbench, typically one per receipt. |
| Kind | Per-file extractor identifier (meal, transport, lodging, airfare, miscellaneous, membership, mileage). |
| Issue | Validation rule the report fails. Non-blocking. |
| Provenance | Source text the tool used to extract a value. |
| CSV | Downloadable file uploaded to Stanford's portal pages. |
| upload_id | Unique ID in the workbench URL. Bookmarkable. |
| IAP | Identity-Aware Proxy. Google's SSO gate for the deployed service. |
