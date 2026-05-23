"""Headless-browser regression tests for the FA-facing UI (Stage 24+B1).

Three test classes:

  TestWorkbenchBrowser        — static workbench load (file://, csv8a)
  TestWorkbenchBrowserB1      — B1 split-extractor cards (live Flask)
  TestWorkbenchBrowserB1Flows — end-to-end FA interaction flows on workbench
  TestUploadFormBrowser       — upload form + localStorage persistence
                                (Stage 11b) + POST-redirect-GET (Stage 11)

Catches the class of regression where a template edit silently
breaks the JS (e.g. an unescaped quote in an inline string) — which
the Python + Rust test suites can't catch, since they don't exercise
JavaScript.

DESIGN — file:// not http://:
  We load the workbench via `file:///path/to/workbench.html` rather
  than spawning a Flask subprocess. Endpoints (POST /edit, /undo,
  /delete-line, GET /undo-available) are covered by the audit script
  + the existing curl-based eyeballs. Playwright's job here is
  narrow: did the JS parse, do the IIFEs wire up, are the FA-visible
  DOM elements present?

  This means fetch() calls in the JS will fail (no Flask), but the
  page still loads + the IIFEs still run + we can assert the DOM
  is wired correctly.

PREREQUISITES:
  ./.venv/bin/pip install playwright
  ./.venv/bin/playwright install chromium

  ~150 MiB browser download. Tests skip themselves (instead of
  failing) when Playwright isn't installed so CI without the
  browser doesn't block PRs on this suite.

USES the .scratch/uploads/csv8a/workbench.html artifact produced by
scripts/stage_eyeball.sh csv8a. If it's missing, the tests skip
themselves with a clear message.
"""

from __future__ import annotations

import json
import subprocess
import sys
import unittest
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKBENCH = REPO_ROOT / ".scratch" / "uploads" / "csv8a" / "workbench.html"

# B1: a second fixture staged by hand for the FA-side UI regression
# (split-extractor for meal/transport). The live-Flask test class
# below expects this directory to contain a freshly-rendered
# workbench.html + reduced/report.json + extractions/*.json + the
# raw receipts. If absent, those tests skip themselves.
B1_FIXTURE = REPO_ROOT / ".scratch" / "uploads" / "b1-eye"
B1_FLASK_URL = "http://localhost:8088"

# B2: a separate fixture for the personal-mileage extractor. Uses a
# synthetic per-doc JSON (no real receipt yet — the FA will drop one
# into receipts/ for the real-Gemini smoke). The synthetic JSON
# exercises the reduce side (derive_mileage_line_amount) + render
# side (Mileage Details card) without depending on Vertex.
B2_FIXTURE = REPO_ROOT / ".scratch" / "uploads" / "b2-eye"

# Stage 18c: import the storage-key constant from local_app_simple
# rather than redeclaring the string here. Without this import, a
# rename in Python would leave the test asserting against the old
# key — silent drift that survives unit tests.
sys.path.insert(0, str(REPO_ROOT / "scripts"))
from local_app_simple import FORM_DRAFT_KEY  # noqa: E402

try:
    from playwright.sync_api import sync_playwright
    PLAYWRIGHT_AVAILABLE = True
except ImportError:
    PLAYWRIGHT_AVAILABLE = False


def _flask_reachable(url: str) -> bool:
    """Quick GET to check Flask is running before we try to load workbench
    over HTTP. Avoids cryptic Playwright timeouts when the user forgot
    to start Flask."""
    try:
        with urllib.request.urlopen(url + "/", timeout=2) as resp:
            return resp.status == 200
    except (urllib.error.URLError, ConnectionRefusedError, TimeoutError):
        return False


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed — run `pip install playwright && "
                     "playwright install chromium`")
@unittest.skipUnless(WORKBENCH.exists(),
                     f"{WORKBENCH} not staged — run scripts/stage_eyeball.sh csv8a first")
class TestWorkbenchBrowser(unittest.TestCase):
    """Each test launches Chromium, navigates to the workbench, runs
    its assertions, closes. ~3-5s per test (browser launch + parse).
    Slow-ish but bounded; total suite under 30s for the few tests we
    have."""

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page()
        # Capture console events so each test can assert on them.
        self._console_errors: list[str] = []
        self._page_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))
        self.page.on("pageerror", lambda exc: self._page_errors.append(str(exc)))

    def tearDown(self):
        self.page.close()

    def _load(self):
        url = f"file://{WORKBENCH.resolve()}"
        # wait_until=domcontentloaded — we don't need image/font loads,
        # just the script tags to have parsed + IIFEs to have run.
        self.page.goto(url, wait_until="domcontentloaded")

    # ─── Test cases ────────────────────────────────────────────────────────

    def test_no_javascript_parse_errors(self):
        """Catches the regression class where a template edit (e.g.
        unescaped quote in an inline string) breaks the JS at parse
        time. A working page should have ZERO page errors."""
        self._load()
        self.assertEqual(self._page_errors, [],
                         f"page parsed with JS errors: {self._page_errors}")

    def test_field_cards_have_data_path_attributes(self):
        """Stage 7 click-to-edit relies on every editable field card
        carrying a data-path attribute. If a Rust render change drops
        the attribute, edits silently break."""
        self._load()
        cards = self.page.query_selector_all('[data-path]')
        self.assertGreater(len(cards), 50,
                           f"expected many editable field cards, got {len(cards)}")

    def test_csv_download_links_render_in_hero(self):
        """Stage 8a+8b: hero should have at least one CSV download link.
        Hidden when count=0; visible otherwise. csv8a has both."""
        self._load()
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0, "expected at least one CSV download link")

    def test_delete_buttons_on_every_transaction_line(self):
        """Stage 23: × delete button on each line summary, data-line-idx
        attribute. Count should equal the number of transaction lines."""
        self._load()
        line_cards = self.page.query_selector_all('details.line-card')
        delete_buttons = self.page.query_selector_all('button.line-delete[data-line-idx]')
        self.assertEqual(len(line_cards), len(delete_buttons),
                         f"every line should have a delete button: "
                         f"{len(line_cards)} lines, {len(delete_buttons)} buttons")

    def test_undo_button_present_in_hero(self):
        """Stage 23: undo button rendered in hero. Hidden by default
        (setupUndoButton JS reveals it on /undo-available poll). For
        file:// load the fetch fails so it stays hidden — we just
        check the element is in the DOM."""
        self._load()
        btn = self.page.query_selector('#undo-button')
        self.assertIsNotNone(btn, "undo button missing from hero")

    def test_evidence_toggle_present(self):
        """Stage 6 evidence toggle. Sanity that the hero markup is
        complete."""
        self._load()
        toggle = self.page.query_selector('#evidence-toggle')
        self.assertIsNotNone(toggle, "evidence-toggle checkbox missing")

    def test_no_console_errors_after_load(self):
        """Filters out the expected fetch failures from setupUndoButton
        (file:// can't fetch /uploads/.../undo-available) and asserts
        no other console errors. If the JS throws elsewhere, this
        catches it."""
        self._load()
        # The undo-button JS catches its own fetch error and just leaves
        # the button hidden — no console.error. If we see any console
        # errors at all, they're unexpected.
        unexpected = [e for e in self._console_errors
                      if "undo-available" not in e]
        self.assertEqual(unexpected, [],
                         f"unexpected console errors: {unexpected}")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B1_FIXTURE.exists(),
                     f"{B1_FIXTURE} not staged — re-run the B1 smoke pipeline")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL} — start it with "
                     "`PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents "
                     "./.venv/bin/python scripts/local_app_simple.py &`")
class TestWorkbenchBrowserB1(unittest.TestCase):
    """B1 live-Flask UI regression: the meal + transport extractors
    were split-call (was single-call until B1) so we could re-add
    pre_tax_amount + tax_amount (meal) and tip + pre_tax + tax
    (transport) after the Stage 9c revert. The workbench renders new
    field cards for those fields; the validator uses them as the
    precise base for the 20% tip cap.

    These tests load the workbench OVER HTTP (not file://) so the
    inline-edit fetch() calls hit a real Flask + the /edit endpoint
    round-trips. csv8a's static-load tests stay file://; this class
    needs the network for interaction coverage."""

    URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b1-eye/edit"
    REPORT_PATH = B1_FIXTURE / "reduced" / "report.json"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1800})
        self._console_errors: list[str] = []
        self._page_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))
        self.page.on("pageerror", lambda exc: self._page_errors.append(str(exc)))

    def tearDown(self):
        self.page.close()

    def _load(self):
        self.page.goto(self.URL, wait_until="networkidle")

    # ─── B1 visual + structural tests ─────────────────────────────────────

    def test_b1_screenshot_for_eyeball(self):
        """Captures a full-page screenshot for human eyeball. Doesn't
        assert much beyond 'the page loaded and had height > 0' — the
        REAL test is what the human sees in the PNG, which the user
        is supposed to look at after this test class runs."""
        self._load()
        out = REPO_ROOT / ".scratch" / "b1-eye" / "workbench-screenshot.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        self.page.screenshot(path=str(out), full_page=True)
        self.assertTrue(out.exists(), "screenshot file not written")
        self.assertGreater(out.stat().st_size, 10_000,
                           "screenshot file suspiciously small")

    def test_b1_meal_card_labels_render(self):
        """B1 meal cards: 'Subtotal (pre-tax)' + 'Tax' + 'Tip' +
        'Alcohol' + 'Has Alcohol' + 'Venue' all show up. If any
        missing, the field-card render in workbench_simple.rs lost
        a render_meal_details line."""
        self._load()
        body = self.page.content()
        for label in ["Venue", "Subtotal (pre-tax)", "Tax", "Tip",
                      "Alcohol", "Has Alcohol"]:
            self.assertIn(f">{label}<", body,
                          f"meal card label '{label}' missing from rendered HTML")

    def test_b1_transport_card_labels_render(self):
        """B1 transport cards: 'Fare (pre-tax)', 'Taxes & Fees', and
        'Driver Tip' all render — these are the fields re-enabled
        for transport in B1."""
        self._load()
        body = self.page.content()
        # `&` gets HTML-escaped to `&amp;` in the rendered output.
        for label in ["Service Provider", "Origin", "Destination",
                      "Fare (pre-tax)", "Taxes &amp; Fees", "Driver Tip"]:
            self.assertIn(f">{label}<", body,
                          f"transport card label '{label}' missing")

    def test_b1_new_field_cards_have_data_path_attrs(self):
        """Click-to-edit needs `data-path` attributes on every editable
        field card. If a render_*_details function emitted the card
        WITHOUT data-path, the card would render but FA clicks would
        do nothing."""
        self._load()
        for path_suffix in [
            "meal_details.pre_tax_amount",
            "meal_details.tax_amount",
            "ground_transport_details.pre_tax_amount",
            "ground_transport_details.tax_amount",
            "ground_transport_details.tip_amount",
        ]:
            els = self.page.query_selector_all(f'[data-path$="{path_suffix}"]')
            self.assertGreater(len(els), 0,
                               f"no data-path element ending in '{path_suffix}'")

    def test_b1_click_pre_tax_opens_inline_editor(self):
        """The Stage 7 click-to-edit flow: clicking a value inside a
        field card swaps the value for an <input>. Verifies the new
        meal_details.pre_tax_amount card participates in that flow
        (it would NOT, if the JS selector excluded the new fields)."""
        self._load()
        # Find the pre_tax_amount card for the first meal (index 1
        # in our b1-eye fixture; line 0 is transport).
        card = self.page.query_selector(
            '[data-path$="meal_details.pre_tax_amount"]'
        )
        self.assertIsNotNone(card, "pre_tax_amount card not found")
        # Click the value text inside the card to open the editor.
        value_el = card.query_selector(".field-value")
        self.assertIsNotNone(value_el, "field-value not found inside card")
        value_el.click()
        # Editor input should appear.
        input_el = card.query_selector("input.inline-edit-input")
        self.assertIsNotNone(input_el,
                             "click did not open inline editor on new field")

    def test_b1_edit_endpoint_round_trip(self):
        """Direct POST to /edit (no browser) for the new pre_tax_amount
        field. Verifies the path walks correctly + coercion accepts
        a float string + the report.json is mutated on disk. This is
        the 'edits don't silently no-op' guarantee."""
        # Snapshot original value from the on-disk report so we can
        # restore at end and compute the new value.
        original_report = json.loads(self.REPORT_PATH.read_text())
        meal_idx = next(
            i for i, line in enumerate(
                original_report["transaction_lines"])
            if line.get("meal_details")
            and line["meal_details"]["pre_tax_amount"]["value"] is not None
        )
        path = f"expense_report.transaction_lines[{meal_idx}].meal_details.pre_tax_amount"
        original_value = original_report["transaction_lines"][meal_idx][
            "meal_details"]["pre_tax_amount"]["value"]
        new_value = round(original_value + 7.77, 2)

        try:
            req = urllib.request.Request(
                self.EDIT_URL,
                data=json.dumps({"path": path, "value": str(new_value)}).encode(),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                self.assertEqual(resp.status, 200,
                                 "edit endpoint did not accept new pre_tax value")
            # Verify it landed on disk.
            updated = json.loads(self.REPORT_PATH.read_text())
            stored = updated["transaction_lines"][meal_idx][
                "meal_details"]["pre_tax_amount"]["value"]
            self.assertEqual(stored, new_value,
                             "/edit did not persist the new pre_tax value")
        finally:
            # Restore original on disk so re-runs are idempotent.
            restore_req = urllib.request.Request(
                self.EDIT_URL,
                data=json.dumps(
                    {"path": path, "value": str(original_value)}
                ).encode(),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            try:
                urllib.request.urlopen(restore_req, timeout=10).close()
            except urllib.error.URLError:
                pass


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B1_FIXTURE.exists(),
                     f"{B1_FIXTURE} not staged")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestWorkbenchBrowserB1Flows(unittest.TestCase):
    """End-to-end FA interaction flows against the live Flask. The
    TestWorkbenchBrowserB1 class above covers STRUCTURE (cards render,
    labels visible). This class covers BEHAVIOR (click does the right
    thing, network round-trips, DOM updates).

    Each test idempotently restores any state it mutates (edits get
    reverted, deletes get undone via the undo endpoint) so re-runs
    don't drift the fixture. Tests do mutate the b1-eye report.json
    on disk briefly; teardown restores."""

    URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b1-eye/edit"
    UNDO_URL = f"{B1_FLASK_URL}/uploads/b1-eye/undo"
    DELETE_URL = f"{B1_FLASK_URL}/uploads/b1-eye/delete-line"
    REPORT_PATH = B1_FIXTURE / "reduced" / "report.json"
    WORKBENCH_PATH = B1_FIXTURE / "workbench.html"
    EDIT_HISTORY_PATH = B1_FIXTURE / "edit_history.json"
    RENDER_BIN = REPO_ROOT / "target" / "debug" / "render_workbench_from_report"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)
        # Snapshot the report so we can restore it before every test
        # (full isolation — delete-line and edit tests both mutate it).
        cls._original_report = cls.REPORT_PATH.read_text()

    @classmethod
    def tearDownClass(cls):
        cls.REPORT_PATH.write_text(cls._original_report)
        cls._render_workbench()
        if cls.EDIT_HISTORY_PATH.exists():
            cls.EDIT_HISTORY_PATH.unlink()
        cls._browser.close()
        cls._playwright.stop()

    @classmethod
    def _render_workbench(cls):
        """Re-render the workbench HTML + CSVs from the current
        report.json on disk. Used to reset the fixture between tests
        (delete-line removes a line from report.json AND re-renders
        workbench; restoring report.json alone leaves a stale HTML)."""
        subprocess.run(
            [
                str(cls.RENDER_BIN),
                "--report", str(cls.REPORT_PATH),
                "--receipts-dir", str(B1_FIXTURE / "extractions"),
                "--out", str(cls.WORKBENCH_PATH),
                "--csv-domestic-out", str(B1_FIXTURE / "lines-domestic.csv"),
                "--csv-foreign-out", str(B1_FIXTURE / "lines-foreign.csv"),
            ],
            check=True,
            capture_output=True,
        )

    def setUp(self):
        # Restore report.json + clear edit_history + re-render workbench
        # so every test starts with the same on-disk state. Without
        # this, test ordering (alphabetical) lets the delete-line test
        # contaminate later tests.
        self.REPORT_PATH.write_text(self._original_report)
        if self.EDIT_HISTORY_PATH.exists():
            self.EDIT_HISTORY_PATH.unlink()
        self._render_workbench()
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1800})
        self._console_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))

    def tearDown(self):
        self.page.close()

    def _load(self):
        self.page.goto(self.URL, wait_until="networkidle")

    # ─── 1. Click-to-edit end-to-end via the browser ─────────────────────

    def test_edit_save_full_browser_flow_updates_value(self):
        """The Stage 7 FA flow end-to-end: click value → type new →
        Enter → page reloads → new value displayed. Asserts the WHOLE
        round-trip works in the browser, not just the endpoint
        (TestWorkbenchBrowserB1's /edit test covers the endpoint
        side; this covers DOM + fetch + reload + re-render)."""
        self._load()
        # Pick the southern-tier meal's tax field (small, visible,
        # easy to assert before/after).
        path = "expense_report.transaction_lines[2].meal_details.tax_amount"
        original = json.loads(self.REPORT_PATH.read_text())[
            "transaction_lines"][2]["meal_details"]["tax_amount"]["value"]
        new_value = round(original + 0.55, 2)
        try:
            card = self.page.query_selector(f'[data-path="{path}"]')
            self.assertIsNotNone(card, f"card for {path} not found")
            card.query_selector(".field-value").click()
            input_el = card.query_selector("input.inline-edit-input")
            self.assertIsNotNone(input_el)
            input_el.fill(str(new_value))
            # Wait for the /edit POST response BEFORE pressing Enter,
            # then trigger Enter — gives us a synchronization point for
            # the JS's double-navigation (location.href hash + reload).
            with self.page.expect_response(
                lambda r: "/edit" in r.url and r.request.method == "POST",
                timeout=10000,
            ):
                input_el.press("Enter")
            # After the POST returns, the JS reloads the page. Wait
            # for the resulting reload by polling for the new value
            # in the .field-value text (which won't show 'Saving…'
            # anymore once the fresh HTML renders).
            self.page.wait_for_function(
                f"() => {{ const el = document.querySelector("
                f"'[data-path=\"{path}\"] .field-value'); "
                f"return el && !el.textContent.includes('Saving'); }}",
                timeout=10000,
            )
            displayed = self.page.query_selector(
                f'[data-path="{path}"] .field-value').text_content()
            self.assertIn(f"{new_value}", displayed,
                          f"page did not show new value after reload: got {displayed!r}")
        finally:
            # Restore so the next test sees the original value.
            urllib.request.urlopen(
                urllib.request.Request(
                    self.EDIT_URL,
                    data=json.dumps({"path": path, "value": str(original)}).encode(),
                    headers={"Content-Type": "application/json"},
                    method="POST",
                ), timeout=10,
            ).close()

    # ─── 2. Delete line full flow ─────────────────────────────────────────

    def test_delete_line_removes_line_from_dom_after_reload(self):
        """Stage 23: ✕ button → confirm dialog → DELETE endpoint →
        reload → line gone. We accept the confirm via the dialog
        handler; without it, click would no-op."""
        self._load()
        before = self.page.query_selector_all("details.line-card")
        self.assertEqual(len(before), 3, "fixture should have 3 lines")
        self.page.on("dialog", lambda d: d.accept())
        # Delete line index 0 (the lyft transport line). After reload,
        # we should have 2 lines.
        btn = self.page.query_selector(
            'button.line-delete[data-line-idx="0"]')
        self.assertIsNotNone(btn, "delete button for line 0 missing")
        with self.page.expect_navigation(wait_until="networkidle"):
            btn.click()
        after = self.page.query_selector_all("details.line-card")
        self.assertEqual(len(after), 2,
                         "expected one fewer line after delete")
        # The remaining lines should be the two meal lines (index 0+1
        # post-delete).
        amounts = " ".join(
            (el.text_content() or "")
            for el in self.page.query_selector_all(".line-amount")
        )
        self.assertNotIn("57.04", amounts,
                         "lyft $57.04 line should be gone after delete")
        # Restore: the deleted line is gone from disk; reload the
        # fixture from the class-level snapshot for subsequent tests.
        self.REPORT_PATH.write_text(self._original_report)

    # ─── 3. Undo button appears after edit; click reverts ───────────────

    def test_undo_button_hidden_before_edits(self):
        """Pre-edit: the undo button is hidden (CSS hidden attribute).
        The JS polls /undo-available; if empty, button stays hidden."""
        self._load()
        btn = self.page.query_selector("#undo-button")
        self.assertIsNotNone(btn, "undo button should exist in DOM")
        # The button has `hidden` attribute set initially. The JS
        # removes it only after /undo-available returns non-empty.
        is_hidden = self.page.evaluate(
            "() => document.getElementById('undo-button').hidden")
        self.assertTrue(is_hidden,
                        "undo button should be hidden when no edits exist")

    def test_undo_button_appears_after_edit_then_reverts(self):
        """Make an edit (POST /edit directly), reload, assert button
        becomes visible. Click it, assert the change reverts on
        disk."""
        path = "expense_report.transaction_lines[2].meal_details.tip_amount"
        original = json.loads(self.REPORT_PATH.read_text())[
            "transaction_lines"][2]["meal_details"]["tip_amount"]["value"]
        try:
            # 1. Edit.
            urllib.request.urlopen(
                urllib.request.Request(
                    self.EDIT_URL,
                    data=json.dumps(
                        {"path": path, "value": str(original + 1.0)}).encode(),
                    headers={"Content-Type": "application/json"},
                    method="POST",
                ), timeout=10,
            ).close()
            # 2. Reload + assert button visible.
            self._load()
            # The JS reveals the button via its own fetch — wait for
            # the network call to settle.
            self.page.wait_for_function(
                "() => !document.getElementById('undo-button').hidden",
                timeout=5000,
            )
            # 3. Click undo + verify the change reverted on disk.
            with self.page.expect_navigation(wait_until="networkidle"):
                self.page.query_selector("#undo-button").click()
            reverted = json.loads(self.REPORT_PATH.read_text())[
                "transaction_lines"][2]["meal_details"]["tip_amount"]["value"]
            self.assertEqual(reverted, original,
                             "undo did not restore the original value")
        finally:
            # Defensive restore (in case undo failed mid-test).
            self.REPORT_PATH.write_text(self._original_report)

    # ─── 4. Evidence toggle on/off ────────────────────────────────────────

    def test_evidence_toggle_adds_body_class_on_check(self):
        """Stage 6: clicking the evidence toggle adds .show-evidence
        to <body>, which the CSS reads to reveal field-evidence
        quotes. Verifies the JS handler is still wired."""
        self._load()
        # Default state: body should NOT have .show-evidence.
        has_class_initial = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertFalse(has_class_initial,
                         "evidence should be hidden by default")
        self.page.click("#evidence-toggle")
        has_class_on = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertTrue(has_class_on,
                        "checking toggle should add .show-evidence to body")
        # Toggle off again.
        self.page.click("#evidence-toggle")
        has_class_off = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertFalse(has_class_off,
                         "unchecking toggle should remove .show-evidence")

    # ─── 5. Issues rail jump scrolls + highlights ─────────────────────────

    def test_issue_jump_link_highlights_target_card(self):
        """Stage 5: clicking an issue-jump link in the rail should
        add .field-card.active to the target card. The CSS draws a
        highlight ring around active cards so the FA can see where
        to act."""
        self._load()
        jumps = self.page.query_selector_all("a.issue-jump")
        # The fixture has 9 missing-required-field issues, so at
        # least one jump link should exist.
        self.assertGreater(len(jumps), 0,
                           "expected issue-jump links in the issues rail")
        jumps[0].click()
        # The JS removes .active from any prior card + adds it to the
        # target. Wait briefly for the click handler to run.
        self.page.wait_for_function(
            "() => document.querySelectorAll('.field-card.active').length === 1",
            timeout=2000,
        )
        active_count = self.page.evaluate(
            "() => document.querySelectorAll('.field-card.active').length")
        self.assertEqual(active_count, 1,
                         "issue-jump should highlight exactly one card")

    # ─── 6. CSV downloads serve real text/csv content ────────────────────

    def test_csv_download_links_serve_csv_content(self):
        """Stage 8a+8b: both CSV download links resolve to real CSV
        files served by Flask. A regression where Flask's static path
        broke (or the file wasn't staged) would 404 here. We check
        both content-type + a sentinel string from the CSV body."""
        self._load()
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0, "expected CSV download links")
        for link in links:
            href = link.get_attribute("href")
            self.assertIsNotNone(href, "link missing href")
            # CSV hrefs in the rendered workbench are relative
            # (e.g. "lines-domestic.csv"); resolve against page URL.
            full_url = urllib.parse.urljoin(self.URL, href)
            with urllib.request.urlopen(full_url, timeout=5) as resp:
                self.assertEqual(resp.status, 200,
                                 f"CSV link {href} returned {resp.status}")
                body = resp.read().decode()
                # Both CSV variants have a comma in the first row
                # (header). If the file is empty or HTML, this'd fail.
                self.assertIn(",", body.split("\n")[0],
                              f"CSV at {href} body doesn't look like CSV: "
                              f"{body[:120]!r}")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestUploadFormBrowser(unittest.TestCase):
    """Tests the FA's upload page (`/`) + persistence behavior.

    Stage 11b localStorage form persistence: typed values (fa_name,
    fa_event_name, fa_bp_who, fa_bp_what, dates) survive a page
    refresh so the FA never has to retype after a pipeline error.

    Stage 11 POST-redirect-GET: a workbench URL is GETable; refreshing
    it doesn't re-submit the form (no 405, no duplicate upload).

    Each test gets a fresh browser context so localStorage is empty
    at start; the JS storage key is `stanford-expense-form-draft-v1`
    (mirrors FORM_DRAFT_KEY in scripts/local_app_simple.py)."""

    HOME_URL = f"{B1_FLASK_URL}/"
    WORKBENCH_URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    # Stage 18c: imported from local_app_simple so a Python-side
    # rename can't silently leave the test stale.
    STORAGE_KEY = FORM_DRAFT_KEY

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        # Fresh context per test = isolated localStorage. Without
        # this, test ordering could leak persisted form drafts
        # between tests and the assertions would be wrong.
        self.context = self._browser.new_context()
        self.page = self.context.new_page()

    def tearDown(self):
        self.context.close()

    # ─── Page structure ──────────────────────────────────────────────────

    def test_upload_form_loads_with_all_fa_fields(self):
        """The FA needs every named field present to fill the form.
        If a template edit dropped a field, this catches it before
        the FA sees a partial form."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        for name in [
            "fa_payee_name", "fa_payee_sunet", "fa_payee_affiliation",
            "fa_event_name", "fa_payment_method",
            "fa_bp_who", "fa_bp_what", "fa_bp_when_from", "fa_bp_when_to",
            "fa_bp_where", "fa_bp_why",
            "file_0", "kind_0",
        ]:
            el = self.page.query_selector(f'[name="{name}"]')
            self.assertIsNotNone(el, f"form field [name={name}] missing")
        # Submit target is /upload (Stage 11 POST-redirect-GET).
        form = self.page.query_selector('form[action="/upload"]')
        self.assertIsNotNone(form,
                             "form should POST to /upload (Stage 11 pattern)")

    # ─── Stage 11b localStorage persistence ──────────────────────────────

    def test_text_fields_persist_across_reload(self):
        """Type into fa_event_name + fa_bp_who, reload, assert values
        restored from localStorage. This is the central Stage 11b
        guarantee: the FA never loses typed work to a refresh."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_event_name"]', "ASPLOS 2026")
        self.page.fill('[name="fa_bp_who"]', "Aditya Sriram")
        self.page.fill('[name="fa_bp_what"]', "Conference travel")
        # The save handler debounces 300ms — give it time before
        # we reload.
        self.page.wait_for_timeout(500)
        self.page.reload(wait_until="domcontentloaded")
        event_val = self.page.input_value('[name="fa_event_name"]')
        who_val = self.page.input_value('[name="fa_bp_who"]')
        what_val = self.page.input_value('[name="fa_bp_what"]')
        self.assertEqual(event_val, "ASPLOS 2026",
                         "fa_event_name should restore from localStorage")
        self.assertEqual(who_val, "Aditya Sriram")
        self.assertEqual(what_val, "Conference travel")

    def test_date_fields_persist_across_reload(self):
        """Dates use the same persistence path. After restoring the
        from-date, the JS fires a `change` event to repopulate
        to-date's default — verify that pipeline still works."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_bp_when_from"]', "2026-03-21")
        self.page.fill('[name="fa_bp_when_to"]', "2026-04-04")
        self.page.wait_for_timeout(500)
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_when_from"]'), "2026-03-21")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_when_to"]'), "2026-04-04")

    def test_file_input_not_persisted_for_security(self):
        """File inputs CANNOT be programmatically set on reload
        (browser security forbids it). Verify the persistence code
        knows to skip them — if it tried, we'd see a console error
        or a wrong value in the file input after reload."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        # Type a text field so the save handler runs (storing
        # something in localStorage); file_0 should NOT be in there.
        self.page.fill('[name="fa_event_name"]', "test event")
        self.page.wait_for_timeout(500)
        snapshot_raw = self.page.evaluate(
            f"() => localStorage.getItem('{self.STORAGE_KEY}')")
        self.assertIsNotNone(snapshot_raw,
                             "localStorage should have form draft")
        snapshot = json.loads(snapshot_raw)
        self.assertNotIn("file_0", snapshot,
                         "file inputs must not be persisted (browser security)")
        self.assertNotIn("kind_0", snapshot,
                         "kind selector ties to file; shouldn't persist alone")
        self.assertIn("fa_event_name", snapshot,
                      "regular text fields SHOULD be persisted")

    def test_persistence_survives_navigation_away_and_back(self):
        """The FA's real-world pain: type some fields → click submit
        → pipeline errors → click 'Try again' (browser back / link)
        → blank form. Stage 11b says localStorage should survive
        navigation. Simulate via navigate-away-and-back."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_event_name"]', "Persistent Event Name")
        self.page.wait_for_timeout(500)
        # Navigate to workbench (any URL that's not /), then back.
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        # Field should be restored.
        self.assertEqual(
            self.page.input_value('[name="fa_event_name"]'),
            "Persistent Event Name",
            "field draft should survive navigation away and back",
        )

    # ─── Stage 11 POST-redirect-GET ─────────────────────────────────────

    def test_workbench_url_is_GETable_and_reloads_cleanly(self):
        """Refresh-on-workbench should be a plain GET (no POST replay).
        Tests that the workbench URL is reloadable — Stage 11 fixed
        the bug where browsers would re-POST the upload on F5."""
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        # Capture all requests during reload to ensure no POST to
        # /upload happens.
        post_requests: list[str] = []
        self.page.on("request", lambda req: (
            post_requests.append(req.url)
            if req.method == "POST" and "/upload" in req.url
            else None
        ))
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(post_requests, [],
                         "refresh on workbench must not re-POST to /upload")

    def test_workbench_url_returns_200_for_direct_navigation(self):
        """Independent of an in-memory upload session, a workbench URL
        should serve cleanly when the FA opens it from a bookmark or
        a shared link days later. Verifies Flask's static-style serve
        of /uploads/<id>/workbench.html."""
        response_status: list[int] = []
        self.page.on("response", lambda resp: (
            response_status.append(resp.status)
            if resp.url == self.WORKBENCH_URL
            else None
        ))
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        self.assertTrue(
            any(s == 200 for s in response_status),
            f"workbench URL should return 200; saw {response_status}",
        )


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B2_FIXTURE.exists(),
                     f"{B2_FIXTURE} not staged — synth-mileage fixture missing")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestWorkbenchBrowserB2(unittest.TestCase):
    """B2 personal-mileage UI regression. Uses a synthetic per-doc
    JSON (Stanford → SFO, 32 mi, 2025-04-15) so we can exercise the
    full reduce + render + JS path without a real Gemini call. When
    a real mileage receipt lands in receipts/, the same tests run
    against the real-Gemini fixture (b2-real or similar) and stay
    deterministic because the assertions key off the receipt's
    metadata, not the exact extracted text."""

    URL = f"{B1_FLASK_URL}/uploads/b2-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b2-eye/edit"
    REPORT_PATH = B2_FIXTURE / "reduced" / "report.json"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1600})
        self._console_errors: list[str] = []
        self._page_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))
        self.page.on("pageerror", lambda exc: self._page_errors.append(str(exc)))

    def tearDown(self):
        self.page.close()

    def _load(self):
        self.page.goto(self.URL, wait_until="networkidle")

    # ─── B2 visual + structural tests ─────────────────────────────────────

    def test_b2_screenshot_for_eyeball(self):
        """Captures a full-page screenshot for human eyeball. Saves to
        .scratch/b2-eye/workbench-screenshot.png. The user is expected
        to look at the PNG after the test run."""
        self._load()
        out = REPO_ROOT / ".scratch" / "b2-eye" / "workbench-screenshot.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        self.page.screenshot(path=str(out), full_page=True)
        self.assertGreater(out.stat().st_size, 10_000)

    def test_b2_no_javascript_parse_errors(self):
        """Same regression guard as the csv8a tests — a template edit
        could break the JS and the mileage workbench would render
        broken. Pageerrors include parse errors + thrown exceptions."""
        self._load()
        self.assertEqual(self._page_errors, [],
                         f"page parsed with JS errors: {self._page_errors}")

    def test_b2_mileage_card_labels_render(self):
        """B2 mileage cards: 'Distance', 'Origin', 'Destination',
        'Trip Date'. If render_mileage_details() drops a field_card
        call, this catches it."""
        self._load()
        body = self.page.content()
        for label in ["Distance", "Origin", "Destination", "Trip Date"]:
            self.assertIn(f">{label}<", body,
                          f"mileage card label '{label}' missing")

    def test_b2_mileage_cards_have_data_path_for_edit(self):
        """All four mileage_details fields must be click-to-editable
        (Stage 7). If a card renders without data-path, the FA can
        see the value but not edit it."""
        self._load()
        for suffix in ["distance_miles", "origin", "destination", "trip_date"]:
            els = self.page.query_selector_all(
                f'[data-path$="mileage_details.{suffix}"]'
            )
            self.assertGreater(len(els), 0,
                               f"no data-path element ending in 'mileage_details.{suffix}'")

    def test_b2_distance_renders_with_mi_suffix(self):
        """render_mileage_details formats distance as '32.0 mi' (not
        '32' or '32.00'). The 'mi' suffix is what tells the FA
        immediately what unit the workbench is using."""
        self._load()
        card = self.page.query_selector(
            '[data-path$="mileage_details.distance_miles"] .field-value'
        )
        self.assertIsNotNone(card, "distance_miles card missing")
        text = card.text_content() or ""
        self.assertIn("mi", text,
                      f"distance should include 'mi' suffix: {text!r}")
        self.assertIn("32", text)

    def test_b2_computed_line_amount_shows_irs_rate_in_confidence_reason(self):
        """The synthetic JSON sets line_amount_usd.value=null; reduction's
        derive_mileage_line_amount fills it with 32 × $0.70 = $22.40
        AND stamps confidence_reason='32.0 mi × IRS 2025 business
        rate ($0.700/mi)'. The workbench shows this reason on hover
        / when evidence toggle is on. This test guards the
        end-to-end derivation chain — if the rate file goes stale OR
        derive_mileage_line_amount stops running, this catches it."""
        self._load()
        # The line amount appears in the line-summary headline AND
        # in the common.line_amount_usd card. Check both for $22.40.
        body = self.page.content()
        self.assertIn("$22.40", body,
                      "computed line_amount_usd of $22.40 not rendered")
        # The confidence reason is in the HTML even when evidence
        # toggle is off (it's rendered in the card metadata).
        self.assertIn("IRS 2025 business rate", body,
                      "derive_mileage_line_amount confidence reason not in HTML")
        self.assertIn("$0.700/mi", body,
                      "IRS rate in confidence reason should match cached value")

    def test_b2_summary_total_matches_derived_line_amount(self):
        """Regression guard for the bug caught during B2 phase 2:
        reduce_to_expense_report used to sum the summary total from
        the RAW receipts (where mileage's line_amount_usd is null),
        producing a $0 summary even though the line showed $22.40.
        The fix: sum from the DERIVED lines. This test asserts the
        hero shows $22.40 — if the bug regresses, the hero would
        show $0.00 or be blank while the line still shows $22.40."""
        self._load()
        body = self.page.content()
        # Hero spending total is in the hero block; assert it's
        # rendered consistently with the line amount. Both $22.40.
        amount_occurrences = body.count("$22.40")
        self.assertGreaterEqual(amount_occurrences, 2,
                                f"expected $22.40 in both line + hero; "
                                f"found {amount_occurrences} occurrence(s)")


if __name__ == "__main__":
    unittest.main()
