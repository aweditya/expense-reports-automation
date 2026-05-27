# FA user guide

How to use the expense-report extraction tool. Audience: a Stanford
CS/EE Faculty Administrator who's never seen the code. No developer
experience needed.

If you hit something this guide doesn't cover, ping the engineering
contact listed in the deployment guide.

---

## What this is

You upload receipts (PDFs, photos, screenshots). The tool reads them,
extracts the fields you'd normally type into Stanford's expense
report portal, and gives you two downloadable CSVs (one per portal
page). You spot-check the values on the way through and fix anything
that's wrong, then upload the CSVs to the Stanford portal as usual.

It is *not* a replacement for the Stanford portal. It is a
data-entry aid that pre-fills the same fields you'd otherwise type
yourself.

---

## 1. Open the tool

Visit the deployed URL (ask the engineering contact — it's behind
Stanford SSO).

After SSO, you land on the **dashboard**: a list of past expense
reports plus a big **+ File a new expense report** button at the
top-right.

- Click the button to start a fresh report (you'll go to the upload
  form at `/new`).
- Click a row in the table to open a previously-filed report's
  workbench.
- Use the **Filter by payee SUNet** box to narrow the table to one
  person's reports.

If you see a Google "this resource is not accessible" page instead of
the dashboard, your account hasn't been granted IAP access yet —
flag this to the engineering contact.

---

## 2. Fill the form

The form has two parts: **fieldset** (info that applies to the whole
report) and **file rows** (one per receipt).

### Required fields

These are starred on the form and the upload won't submit without
them:

- **Payee SUNet ID** — the SUNet of the person being reimbursed
  (2–16 chars, letters/digits/underscore/hyphen)
- **Event name** — short trip / event label (e.g. "ASPLOS 2026")
- **Business purpose** — Who / What / When / Where / Why

The remaining fieldset values (payee full name, affiliation,
authorized-by, payment method, foreign activity type if any) are
optional but most of them help downstream; fill what you know.

### File rows

Each row is one receipt: a file input + a **kind** dropdown that
tells the tool what type of receipt it is.

Kinds:

| Kind | What to pick it for |
|---|---|
| **Meal Receipt** | Restaurant bill, food order |
| **Ground Transport** | Uber, Lyft, taxi, train, bus, MVG card, parking |
| **Lodging Folio** | Hotel itemized bill / Airbnb invoice |
| **Airfare / Flight Ticket** | Flight booking confirmation (Egencia, Kiwi, airline direct) |
| **Miscellaneous** | Posters, printing, conference dinner per-person bills |
| **Membership Dues** | ACM, IEEE, USENIX membership renewals |
| **Personal Mileage** | Screenshot of Google Maps driving distance (we apply IRS rate) |

Click **+ Add another file** to get more rows. Drag-drop or click the
file inputs to attach the actual files. Allowed file types: PDF, PNG,
JPG, HEIC, WebP.

If you upload a HEIC photo from an iPhone, the tool converts it
automatically — you don't need to do anything special.

### Hit submit

The form will refuse to submit if a required field is missing
(browser highlights the blank field).

---

## 3. Watch the progress page

After submit, you'll land on a progress page that streams updates as
the tool processes each file. You'll see:

- A blue progress bar that fills up phase-by-phase (extract → reduce
  → fx → render → done)
- A per-file list showing which file is currently being processed
- A "done" chime when everything finishes (1-second delay, then
  auto-redirect to the workbench)

**You can stay on this page** — it auto-redirects when done.
**Refreshing is safe** if you need to.
**You can close the tab** — your work isn't lost. When you come back
to the URL, the progress page reconnects and shows the current state.

If one receipt fails to extract, the others keep going. The failed
one shows up in the workbench as a line that needs your attention
(usually a friendly error like "couldn't read the image — try a
clearer photo").

Total time: roughly 30 seconds per receipt. A 5-receipt batch
takes ~3 minutes.

---

## 4. Review the workbench

The workbench is where you spot-check what the tool extracted, fix
mistakes, and grab the CSVs. The page has three regions:

### Hero (top)

- **Title** with line count: e.g. "Review & File (5 lines)"
- **Status pill**: "5 lines · 2 to review" — turns green when zero
  issues remain
- **Payee + event subtitle** so you remember which report you're on
- **Download buttons**: one CSV per Stanford portal page (Domestic
  and/or Foreign — only the ones with data appear)
- **Show extraction provenance** checkbox (see Provenance below)
- **Undo last edit** button (only visible when you've made edits)
- **+ Add more receipts** button (opens a modal — see §6)

### Left rail — Issues

Validation rules that fired. Each issue is a card you can click to
jump to the field it's about. Examples:

- "Tip exceeds 20% of pre-tax + tax" (meal)
- "Date falls outside trip window" (trip window was your business
  purpose When/From–To)
- "Missing required field: X"

You can dismiss an issue card after reviewing it (no persistence —
refresh wipes dismissals; treat them as acknowledgements, not data).

### Main column — line cards

Each line card is one extracted receipt with its fields.

- **Edit a value**: click the value text → it turns into an input →
  edit → Enter to save (Escape to cancel). The edit hits the server
  immediately + persists to durable storage.
- **Delete a line**: click the trash icon on a card. Confirms before
  deleting because indices shift (your undo history is cleared on
  delete).
- **Undo your last edit**: click the **Undo last edit** button in
  the hero. Reverts the most recent edit. History up to 20 edits
  back.

### Provenance ("Show extraction provenance" toggle)

Each value the tool extracted has an evidence trail: which receipt,
which page, what verbatim text it pulled from. Toggle the checkbox
in the hero to reveal it.

Click the **eye** icon on any field card to open the **spot-check
panel** — a side panel that shows the source PDF/image with the
extracted region highlighted (a yellow halo around the relevant
text). Lets you verify "is the tool reading the right number?" in
two clicks.

---

## 5. Download the CSVs

Two CSVs, one per Stanford portal page:

- **`lines-domestic.csv`** — Domestic Expense Lines page (7 columns,
  plain expense-type strings)
- **`lines-foreign.csv`** — Foreign Expense Lines page (20 columns
  including airfare/lodging detail blocks)

The hero only shows the download button for whichever CSVs have data
rows. If your trip was domestic-only, you'll just see the domestic
download.

Open the relevant Stanford portal page, click **Browse**, pick the
CSV, click **Load**. The portal validates the file; if everything
checks out, **Import** appears and you proceed.

---

## 6. Add more receipts later

You uploaded 3 today and realize 2 more belong on the same report?

1. Open the workbench URL (bookmarked or from the past-reports
   listing — see §7).
2. Click **+ Add more receipts** in the hero.
3. A modal opens. Pick the new files + kinds, click **Process**.
4. The progress page shows the NEW files' extraction (the existing
   ones stay as-is). When done, the workbench refreshes with the
   combined report.

Your prior edits to existing line values are preserved. The undo
stack resets (line numbers may have shifted).

---

## 7. Come back later

The workbench URL is **bookmarkable**. Save it. Come back tomorrow,
next week, three months from now — it still works, as long as it's
been less than 90 days since the report was created (after that, our
durable storage cleans it up automatically).

(Past-reports listing UI is coming. Until it ships, save your URLs.)

---

## 8. What to do if something looks wrong

| Symptom | What to do |
|---|---|
| Tool extracted a wrong number | Click the value → edit → Enter. The edit persists; downstream CSVs reflect the change immediately. |
| Tool extracted a wrong currency / expense type | Same as above — click + edit. Dropdowns show valid options. |
| A whole receipt didn't get parsed (line card missing) | Check the progress page or the file's per-file status. If extraction failed, the issue card will explain why (e.g. "image too blurry"). Re-take the photo + add it via §6. |
| You uploaded the wrong kind for a receipt | Today you can't change the kind in-place — delete the line and re-add the receipt with the correct kind. |
| Validation issue you disagree with | The rule is FA-tunable; flag it to engineering with the upload_id + line number. The rules table is `schema.yaml` + `src/validator_typed.rs`. |
| The page is loading forever | Refresh — it's safe. If still stuck, the URL might be from a deploy that wiped state; flag to engineering with the upload_id. |
| You see a Google "Forbidden" page | Your IAP grant lapsed or your SSO session expired. Re-sign-in usually fixes it. |

---

## Glossary

| Term | Meaning |
|---|---|
| **Workbench** | The HTML page you review after extraction completes. |
| **Line card** | One transaction line on the workbench (typically one per receipt). |
| **Kind** | Which extractor we run on a file (meal / transport / lodging / airfare / miscellaneous / membership / mileage). |
| **Issue** | A validation rule the report fails. Left-rail panel. Non-blocking; you decide what to do. |
| **Provenance / evidence** | The verbatim source text we used to extract a value. |
| **CSV** | The two downloadable files you upload to Stanford's portal pages. |
| **upload_id** | The unique ID in your workbench URL. Stable; bookmarkable. |
| **IAP** | Google's Identity-Aware Proxy — what gates access via SSO. |
